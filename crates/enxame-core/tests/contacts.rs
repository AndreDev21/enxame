#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integração: nós in-process, sem relay e sem rede externa.

use std::time::Duration;

use enxame_core::{AddOutcome, Config, Enxame, Event, FileKeyStore, PathKind};
use iroh::{endpoint::presets, Endpoint, EndpointAddr, RelayMode, TransportAddr};
use tempfile::TempDir;

struct TestNode {
    node: Enxame,
    _dir: TempDir,
}

async fn node(name: &str) -> TestNode {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::for_tests(dir.path().to_path_buf(), name);
    let store = FileKeyStore::new(dir.path().join("identity.key"));
    let node = Enxame::start(cfg, &store).await.unwrap();
    TestNode { node, _dir: dir }
}

/// Aceita o primeiro pedido que chegar em `host`.
fn auto_respond(host: &Enxame, accept: bool) -> tokio::task::JoinHandle<()> {
    let mut events = host.subscribe();
    let host = host.clone();
    tokio::spawn(async move {
        while let Ok(ev) = events.recv().await {
            if let Event::ContactRequested { request_id, .. } = ev {
                host.respond_contact(request_id, accept).unwrap();
                break;
            }
        }
    })
}

async fn befriend(a: &Enxame, b: &Enxame) {
    let responder = auto_respond(a, true);
    let ticket = a.invite_ticket().await.unwrap().to_string();
    match tokio::time::timeout(Duration::from_secs(20), b.add_contact(&ticket))
        .await
        .unwrap()
        .unwrap()
    {
        AddOutcome::Accepted(_) => {}
        other => panic!("esperado Accepted, veio {other:?}"),
    }
    responder.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn mutual_acceptance_makes_both_sides_contacts() {
    let a = node("alice").await;
    let b = node("bob").await;
    befriend(&a.node, &b.node).await;

    let a_contacts = a.node.contacts();
    let b_contacts = b.node.contacts();
    assert_eq!(a_contacts.len(), 1);
    assert_eq!(b_contacts.len(), 1);
    assert!(a_contacts[0].is_mutual() && b_contacts[0].is_mutual());
    assert_eq!(a_contacts[0].display_name, "bob");
    assert_eq!(b_contacts[0].display_name, "alice");
    assert_eq!(a_contacts[0].identity_pk, b.node.identity_pk());
    assert!(a.node.pending_requests().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_request_leaves_no_contact() {
    let a = node("alice").await;
    let b = node("bob").await;
    let responder = auto_respond(&a.node, false);
    let ticket = a.node.invite_ticket().await.unwrap().to_string();
    let out = tokio::time::timeout(Duration::from_secs(20), b.node.add_contact(&ticket))
        .await
        .unwrap()
        .unwrap();
    responder.await.unwrap();
    assert_eq!(out, AddOutcome::Rejected);
    assert!(a.node.contacts().is_empty());
    assert!(b.node.contacts().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn contacts_and_keys_survive_restart() {
    let a = node("alice").await;
    let b = node("bob").await;
    befriend(&a.node, &b.node).await;
    let pk = b.node.identity_pk();
    b.node.shutdown().await;

    let cfg = Config::for_tests(b._dir.path().to_path_buf(), "bob");
    let store = FileKeyStore::new(b._dir.path().join("identity.key"));
    let b2 = Enxame::start(cfg, &store).await.unwrap();
    assert_eq!(b2.identity_pk(), pk);
    assert_eq!(b2.contacts().len(), 1);
    assert!(b2.contacts()[0].is_mutual());
}

#[tokio::test(flavor = "multi_thread")]
async fn diag_reports_direct_path_and_rtt_between_contacts() {
    let a = node("alice").await;
    let b = node("bob").await;
    befriend(&a.node, &b.node).await;

    let report = tokio::time::timeout(
        Duration::from_secs(60),
        b.node.diag(Some("alice"), Duration::from_secs(10)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(report.peers.len(), 1);
    let peer = &report.peers[0];
    assert_eq!(peer.error, None, "{peer:?}");
    assert_eq!(peer.protocol_version, Some(1));
    let m = peer.metrics.as_ref().unwrap();
    assert_eq!(m.kind, PathKind::Direct, "{m:?}");
    assert!(m.rtt.is_some());
    assert!(m.time_to_direct.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn strangers_get_no_sync_session() {
    let a = node("alice").await;
    let addr = loopback_addr(&a.node);

    let stranger = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .unwrap();
    let conn = stranger.connect(addr, b"enxame/sync/1").await.unwrap();
    let reason = tokio::time::timeout(Duration::from_secs(10), conn.closed())
        .await
        .expect("o nó deveria fechar a conexão");
    assert!(format!("{reason:?}").contains("403"), "{reason:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn forged_ticket_does_not_create_a_contact() {
    let a = node("alice").await;
    let b = node("bob").await;
    let mut forged: enxame_core::InviteTicket = a.node.invite_ticket().await.unwrap();
    forged.display_name = "mallory".into(); // quebra a assinatura
    let err = b.node.add_contact(&forged.to_string()).await.unwrap_err();
    assert!(err.to_string().contains("ticket inválido"), "{err}");
    assert!(b.node.contacts().is_empty());
    assert!(a.node.pending_requests().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn own_ticket_is_refused() {
    let a = node("alice").await;
    let t = a.node.invite_ticket().await.unwrap().to_string();
    assert!(a.node.add_contact(&t).await.is_err());
}

fn loopback_addr(n: &Enxame) -> EndpointAddr {
    let addr = n.endpoint_addr();
    let ips = addr.ip_addrs().copied().collect::<Vec<_>>();
    assert!(!ips.is_empty(), "endpoint sem endereços IP: {addr:?}");
    EndpointAddr::from_parts(addr.id, ips.into_iter().map(TransportAddr::Ip))
}

/// Sem transporte IP, o único caminho possível é o relay (relay local de teste).
#[tokio::test(flavor = "multi_thread")]
async fn relay_only_nodes_connect_and_diag_reports_relay() {
    use enxame_core::Identity;
    use iroh::tls::CaTlsConfig;

    let (relay_map, _url, _guard) = iroh::test_utils::run_relay_server().await.unwrap();

    async fn relay_node(name: &str, map: &iroh::RelayMap) -> TestNode {
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
        TestNode { node, _dir: dir }
    }

    let a = relay_node("alice", &relay_map).await;
    let b = relay_node("bob", &relay_map).await;
    befriend(&a.node, &b.node).await;

    let report = tokio::time::timeout(
        Duration::from_secs(60),
        b.node.diag(Some("alice"), Duration::from_secs(10)),
    )
    .await
    .unwrap()
    .unwrap();
    let peer = &report.peers[0];
    assert_eq!(peer.error, None, "{peer:?}");
    assert_eq!(peer.metrics.as_ref().unwrap().kind, PathKind::Relay);
}
