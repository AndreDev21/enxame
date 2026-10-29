#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Ponte de mídia (Fase 7.2, etapa 2, ADR 0015): um cliente TURN de verdade fala com o TURN local
//! de cada nó, e os pacotes cruzam a conexão `enxame/media/1` do iroh por datagramas QUIC.

use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};

use enxame_core::{AddOutcome, BridgeRole, Config, Enxame, Event, FileKeyStore, MediaBridge};
use tempfile::TempDir;
use tokio::net::UdpSocket;
use turn::client::{Client, ClientConfig};
use webrtc_util::Conn;

async fn node(name: &str) -> (Enxame, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (start_at(dir.path(), name).await, dir)
}

async fn start_at(dir: &Path, name: &str) -> Enxame {
    let cfg = Config::for_tests(dir.to_path_buf(), name);
    let store = FileKeyStore::new(dir.join("identity.key"));
    Enxame::start(cfg, &store).await.unwrap()
}

async fn befriend(a: &Enxame, b: &Enxame) {
    let mut events = a.subscribe();
    let host = a.clone();
    let responder = tokio::spawn(async move {
        while let Ok(ev) = events.recv().await {
            if let Event::ContactRequested { request_id, .. } = ev {
                host.respond_contact(request_id, true).unwrap();
                break;
            }
        }
    });
    let ticket = a.invite_ticket().await.unwrap().to_string();
    let out = tokio::time::timeout(Duration::from_secs(20), b.add_contact(&ticket))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(out, AddOutcome::Accepted(_)));
    responder.await.unwrap();
}

const CALLER: &str = "198.18.0.1:40000";
const CALLEE: &str = "198.18.0.2:40000";

fn turn_addr(b: &MediaBridge) -> String {
    b.url
        .strip_prefix("turn:")
        .and_then(|s| s.split('?').next())
        .unwrap()
        .to_string()
}

async fn client(b: &MediaBridge, password: &str) -> Client {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let c = Client::new(ClientConfig {
        stun_serv_addr: turn_addr(b),
        turn_serv_addr: turn_addr(b),
        username: b.username.clone(),
        password: password.to_string(),
        realm: "enxame".to_string(),
        software: String::new(),
        rto_in_ms: 0,
        conn: Arc::new(sock),
        vnet: None,
    })
    .await
    .unwrap();
    c.listen().await.unwrap();
    c
}

async fn linked(b: &MediaBridge) {
    for _ in 0..200 {
        if b.is_linked() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("a conexão de mídia não abriu");
}

/// Dois nós amigos com a ponte já ligada: (alice chamadora, bob atendente).
async fn pair() -> (Enxame, Enxame, MediaBridge, MediaBridge, Vec<TempDir>) {
    let ((a, da), (b, db)) = (node("alice").await, node("bob").await);
    befriend(&a, &b).await;
    // O atendente sobe primeiro (como no app: antes de responder `accept`).
    let bridge_b = b
        .media_bridge_start("alice", BridgeRole::Callee)
        .await
        .unwrap();
    let bridge_a = a
        .media_bridge_start("bob", BridgeRole::Caller)
        .await
        .unwrap();
    linked(&bridge_a).await;
    linked(&bridge_b).await;
    (a, b, bridge_a, bridge_b, vec![da, db])
}

#[tokio::test(flavor = "multi_thread")]
async fn packets_cross_the_bridge_both_ways_through_real_turn_clients() {
    let (_a, _b, bridge_a, bridge_b, _dirs) = pair().await;
    let (ca, cb) = (
        client(&bridge_a, &bridge_a.credential).await,
        client(&bridge_b, &bridge_b.credential).await,
    );
    let (ra, rb) = (ca.allocate().await.unwrap(), cb.allocate().await.unwrap());
    // Os endereços "relay" anunciados são os virtuais de cada papel.
    assert_eq!(ra.local_addr().unwrap().to_string(), CALLER);
    assert_eq!(rb.local_addr().unwrap().to_string(), CALLEE);
    let (caller, callee): (SocketAddr, SocketAddr) =
        (CALLER.parse().unwrap(), CALLEE.parse().unwrap());

    // Cada lado cria a permissão para o outro ao enviar (como o ICE faz com os candidatos).
    ra.send_to(b"hello", callee).await.unwrap();
    rb.send_to(b"hello", caller).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // 50 pacotes de 900 B em cada sentido, com conteúdo identificável.
    for i in 0u8..50 {
        let mut p = vec![i; 900];
        p[0] = i;
        ra.send_to(&p, callee).await.unwrap();
        rb.send_to(&p, caller).await.unwrap();
    }
    let mut got_b = std::collections::HashSet::new();
    let mut got_a = std::collections::HashSet::new();
    let mut buf = vec![0u8; 2000];
    for _ in 0..200 {
        if let Ok(Ok((n, from))) =
            tokio::time::timeout(Duration::from_millis(200), rb.recv_from(&mut buf)).await
        {
            if n == 900 {
                assert_eq!(from, caller);
                got_b.insert(buf[0]);
            }
        }
        if let Ok(Ok((n, from))) =
            tokio::time::timeout(Duration::from_millis(200), ra.recv_from(&mut buf)).await
        {
            if n == 900 {
                assert_eq!(from, callee);
                got_a.insert(buf[0]);
            }
        }
        if got_a.len() >= 45 && got_b.len() >= 45 {
            break;
        }
    }
    assert!(got_b.len() >= 45, "bob recebeu {} de 50", got_b.len());
    assert!(got_a.len() >= 45, "alice recebeu {} de 50", got_a.len());
    let (sa, sb) = (bridge_a.stats(), bridge_b.stats());
    assert!(sa.sent >= 50 && sa.received >= 45, "{sa:?}");
    assert!(sb.sent >= 50 && sb.received >= 45, "{sb:?}");
    assert_eq!(sa.dropped_not_ready + sa.dropped_bad_frame, 0, "{sa:?}");
    // In-process o caminho é direto (IP), não por relay.
    assert_eq!(bridge_a.is_direct(), Some(true));
    bridge_a.close().await;
    bridge_b.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_first_allocation_is_granted_and_wrong_credentials_are_refused() {
    let (_a, _b, bridge_a, bridge_b, _dirs) = pair().await;
    let c1 = client(&bridge_a, &bridge_a.credential).await;
    let _r1 = c1.allocate().await.unwrap();
    let c2 = client(&bridge_a, &bridge_a.credential).await;
    assert!(
        c2.allocate().await.is_err(),
        "a 2ª alocação deveria ser recusada"
    );
    let bad = client(&bridge_b, "senha-errada").await;
    assert!(
        bad.allocate().await.is_err(),
        "senha errada deveria ser recusada"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bridge_needs_a_mutual_contact_and_a_waiting_callee() {
    let ((a, _da), (b, _db), (c, _dc)) =
        (node("alice").await, node("bob").await, node("carol").await);
    befriend(&a, &b).await;
    // Contato desconhecido: erro na hora.
    assert!(a
        .media_bridge_start("carol", BridgeRole::Caller)
        .await
        .is_err());
    // Bob não pediu ponte: a conexão de alice é recusada e a ponte dela nunca liga.
    let bridge_a = a
        .media_bridge_start("bob", BridgeRole::Caller)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        !bridge_a.is_linked(),
        "sem atendente esperando, a ponte não deveria ligar"
    );
    // Carol não é contato de ninguém: nem sequer resolve.
    assert!(c
        .media_bridge_start("alice", BridgeRole::Callee)
        .await
        .is_err());
}

/// Exploração (Fase 7.2, etapa 3): quanto cabe num datagrama e como a descoberta de MTU evolui.
/// `cargo test -p enxame-core --test media -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn explore_max_datagram_size_over_time() {
    let (_a, _b, bridge_a, bridge_b, _dirs) = pair().await;
    let (ca, cb) = (
        client(&bridge_a, &bridge_a.credential).await,
        client(&bridge_b, &bridge_b.credential).await,
    );
    let (ra, rb) = (ca.allocate().await.unwrap(), cb.allocate().await.unwrap());
    let (caller, callee): (SocketAddr, SocketAddr) =
        (CALLER.parse().unwrap(), CALLEE.parse().unwrap());
    ra.send_to(b"x", callee).await.unwrap();
    rb.send_to(b"x", caller).await.unwrap();
    let t0 = std::time::Instant::now();
    for step in 0..20 {
        // tráfego de 1000 B, 1 pacote a cada 20 ms, para a descoberta de MTU ter o que sondar
        for _ in 0..10 {
            ra.send_to(&[7u8; 1000], callee).await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        println!(
            "t={:>5} ms  max_datagram A={:?} B={:?}  stats A={:?}",
            t0.elapsed().as_millis(),
            bridge_a.max_datagram_size(),
            bridge_b.max_datagram_size(),
            bridge_a.stats()
        );
        let _ = step;
    }
}

/// Sem transporte IP o único caminho é o relay (relay local de teste): mede quanto cabe num
/// datagrama por esse caminho (Fase 7.2, etapa 3) e se um pacote RTP/SRTP típico passa inteiro.
#[tokio::test(flavor = "multi_thread")]
async fn relay_only_path_carries_an_srtp_sized_packet() {
    use enxame_core::Identity;
    use iroh::{endpoint::presets, tls::CaTlsConfig, Endpoint, RelayMode};

    let (relay_map, _url, _guard) = iroh::test_utils::run_relay_server().await.unwrap();

    async fn relay_node(name: &str, map: &iroh::RelayMap) -> (Enxame, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::for_tests(dir.path().to_path_buf(), name);
        let store = FileKeyStore::new(dir.path().join("identity.key"));
        let (identity, _) = Identity::load_or_create(&store).unwrap();
        let endpoint = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Custom(map.clone()))
            .ca_tls_config(CaTlsConfig::insecure_skip_verify())
            .clear_ip_transports()
            .secret_key(identity.device_secret().clone())
            .bind()
            .await
            .unwrap();
        endpoint.online().await;
        let node = Enxame::start_with_endpoint(cfg, identity, "teste", endpoint)
            .await
            .unwrap();
        (node, dir)
    }

    let ((a, _da), (b, _db)) = (
        relay_node("alice", &relay_map).await,
        relay_node("bob", &relay_map).await,
    );
    befriend(&a, &b).await;
    let bridge_b = b
        .media_bridge_start("alice", BridgeRole::Callee)
        .await
        .unwrap();
    let bridge_a = a
        .media_bridge_start("bob", BridgeRole::Caller)
        .await
        .unwrap();
    linked(&bridge_a).await;
    linked(&bridge_b).await;
    assert_eq!(bridge_a.is_direct(), Some(false), "devia ir pelo relay");

    let (ca, cb) = (
        client(&bridge_a, &bridge_a.credential).await,
        client(&bridge_b, &bridge_b.credential).await,
    );
    let (ra, rb) = (ca.allocate().await.unwrap(), cb.allocate().await.unwrap());
    let (caller, callee): (SocketAddr, SocketAddr) =
        (CALLER.parse().unwrap(), CALLEE.parse().unwrap());
    ra.send_to(b"hello", callee).await.unwrap();
    rb.send_to(b"hello", caller).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    println!(
        "relay: max_datagram A={:?} B={:?}",
        bridge_a.max_datagram_size(),
        bridge_b.max_datagram_size()
    );
    // 1232 B = RTP de 1200 B + 32 B de SRTP/TURN; é o pior caso típico do libwebrtc.
    let mut buf = vec![0u8; 2000];
    // O "hello" de aquecimento ainda está na fila do bob.
    let (n, _) = rb.recv_from(&mut buf).await.unwrap();
    assert_eq!(n, 5);
    for size in [200usize, 900, 1200, 1232, 1300] {
        let before = bridge_a.stats();
        let mut got = 0;
        for _ in 0..10 {
            ra.send_to(&vec![9u8; size], callee).await.unwrap();
            if let Ok(Ok((n, _))) =
                tokio::time::timeout(Duration::from_millis(500), rb.recv_from(&mut buf)).await
            {
                if n == size {
                    got += 1;
                }
            }
        }
        let st = bridge_a.stats();
        println!(
            "relay: {size:>4} B -> chegaram {got}/10, descartados por tamanho {}",
            st.dropped_too_large - before.dropped_too_large
        );
        if size <= 1232 {
            assert_eq!(got, 10, "pacote de {size} B não passou pelo relay");
        }
    }
    println!("relay: A={:?} B={:?}", bridge_a.stats(), bridge_b.stats());
    bridge_a.close().await;
    bridge_b.close().await;
}
