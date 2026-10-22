#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integração da Fase 4: envio e download de arquivos entre nós Enxame (QUIC real em localhost,
//! sem relay e sem rede externa).

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use enxame_core::{AddOutcome, Config, Enxame, Event, FileEntry, FileKeyStore, FileRef};
use tempfile::TempDir;

struct TestNode {
    node: Enxame,
    dir: TempDir,
    cfg: Config,
}

impl TestNode {
    /// Reinicia com a mesma identidade e os mesmos dados (porta nova).
    async fn restart(&mut self) {
        self.node.shutdown().await;
        let store = FileKeyStore::new(self.dir.path().join("identity.key"));
        self.node = Enxame::start(self.cfg.clone(), &store).await.unwrap();
    }
}

async fn node_with(name: &str, tweak: impl FnOnce(&mut Config)) -> TestNode {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::for_tests(dir.path().to_path_buf(), name);
    tweak(&mut cfg);
    let store = FileKeyStore::new(dir.path().join("identity.key"));
    let node = Enxame::start(cfg.clone(), &store).await.unwrap();
    TestNode { node, dir, cfg }
}

/// Nó `peer`: semeia o que baixou (nós novos começam em `Leaf`, que não semeia).
async fn peer(name: &str) -> TestNode {
    node_with(name, |c| c.role = "peer".into()).await
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

/// Bytes pseudoaleatórios determinísticos (xorshift), com um marcador legível no começo.
fn write_file(dir: &Path, name: &str, len: usize, marker: &str) -> PathBuf {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ len as u64;
    let mut data = Vec::with_capacity(len);
    data.extend_from_slice(marker.as_bytes());
    while data.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        data.extend_from_slice(&x.to_le_bytes());
    }
    data.truncate(len);
    let p = dir.join(name);
    std::fs::write(&p, data).unwrap();
    p
}

fn same_content(a: &Path, b: &Path) -> bool {
    std::fs::read(a).unwrap() == std::fs::read(b).unwrap()
}

async fn files_of(n: &TestNode, with: &str) -> Vec<FileEntry> {
    n.node.files(with).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_travels_encrypted_and_arrives_intact() {
    let (a, b) = (peer("alice").await, peer("bob").await);
    befriend(&a.node, &b.node).await;
    let marker = "SEGREDO-ENXAME-MARCADOR-1234567890";
    let src = write_file(
        a.dir.path(),
        "relatório final.bin",
        20 * 1024 * 1024 + 321,
        marker,
    );

    let sent = a.node.send_file("bob", &src).await.unwrap();
    assert!(sent.messages[0].synced, "{sent:?}");
    assert_eq!(sent.name, "relatório final.bin");
    assert_eq!(sent.size, 20 * 1024 * 1024 + 321);

    // O que está no disco dos blobs é só ciphertext: o marcador em claro não aparece.
    let needle = marker.as_bytes();
    let mut scanned = 0;
    for entry in walk(&a.dir.path().join("blobs")) {
        let bytes = std::fs::read(&entry).unwrap();
        scanned += bytes.len();
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle),
            "texto em claro em {entry:?}"
        );
    }
    assert!(
        scanned > 20 * 1024 * 1024,
        "os blobs deveriam estar em disco ({scanned} bytes)"
    );

    // B vê o arquivo como mensagem, antes de baixar.
    let files = files_of(&b, "alice").await;
    assert_eq!(files.len(), 1);
    assert!(!files[0].from_me && !files[0].available);
    assert_eq!(files[0].name, "relatório final.bin");
    let hist = b.node.history("alice", 10).await.unwrap();
    assert!(
        hist[0].text.starts_with("📎 relatório final.bin"),
        "{:?}",
        hist[0].text
    );
    assert_eq!(hist[0].file.as_ref().unwrap().size, sent.size);

    let mut events = b.node.subscribe();
    let out = b.dir.path().join("recebidos");
    let got = tokio::time::timeout(
        Duration::from_secs(60),
        b.node.download_file("alice", "1", Some(out.clone())),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(got.size, sent.size);
    assert!(got.path.starts_with(&out) && got.path.ends_with("relatório final.bin"));
    assert!(same_content(&src, &got.path), "conteúdo diferente");
    // Houve eventos de progresso, terminando em 100 %.
    let mut last = None;
    loop {
        match events.try_recv() {
            Ok(Event::TransferProgress {
                done,
                total,
                upload: false,
                ..
            }) => last = Some((done, total)),
            Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
            Err(_) => break,
        }
    }
    let (done, total) = last.expect("sem TransferProgress");
    assert_eq!(done, total);

    // Depois de baixar, B tem o arquivo completo e baixar de novo não precisa de rede.
    assert!(files_of(&b, "alice").await[0].available);
    a.node.shutdown().await;
    let again = b
        .node
        .download_file("alice", "1", Some(out.clone()))
        .await
        .unwrap();
    assert!(same_content(&src, &again.path));
    assert_ne!(again.path, got.path, "não sobrescreve: ganha um sufixo");
    assert!(again.path.to_string_lossy().contains("(1)"));
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn two_contacts_serve_the_same_download_at_the_same_time() {
    let (a, b, c) = (peer("alice").await, peer("bob").await, peer("carol").await);
    befriend(&a.node, &b.node).await;
    befriend(&a.node, &c.node).await;
    befriend(&b.node, &c.node).await;
    let src = write_file(a.dir.path(), "grande.bin", 96 * 1024 * 1024, "g");
    // O MESMO blob e a MESMA chave vão para os dois (um único arquivo cifrado).
    let sent = a.node.send_file_to(&["bob", "carol"], &src).await.unwrap();
    assert!(sent.messages.iter().all(|m| m.synced));

    // Carol baixa primeiro, só de Alice, e passa a semear.
    let t = Instant::now();
    let cgot = c
        .node
        .download_file("alice", "1", Some(c.dir.path().join("dl")))
        .await
        .unwrap();
    assert!(same_content(&src, &cgot.path));
    let first = t.elapsed();

    // Bob baixa de Alice **e** de Carol ao mesmo tempo.
    let (a0, c0) = (a.node.file_bytes_sent(), c.node.file_bytes_sent());
    let bgot = tokio::time::timeout(
        Duration::from_secs(90),
        b.node
            .download_file("alice", "1", Some(b.dir.path().join("dl"))),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(same_content(&src, &bgot.path));
    let (from_a, from_c) = (a.node.file_bytes_sent() - a0, c.node.file_bytes_sent() - c0);
    let enc = enxame_core_enc_len(sent.size);
    eprintln!(
        "1º download {first:?}; Bob recebeu {from_a} de Alice e {from_c} de Carol ({enc} no total)"
    );
    assert!(from_a > enc / 4, "Alice serviu só {from_a} de {enc}");
    assert!(from_c > enc / 4, "Carol serviu só {from_c} de {enc}");
    assert!(from_a + from_c >= enc, "somaram menos que o arquivo");
    // O que Carol serviu a Bob entra na contribuição dela (não era arquivo dela).
    let summary = c.node.contribution_summary().await.unwrap();
    assert!(summary.bytes_served > 0, "{summary:?}");
}

/// Tamanho do ciphertext de um arquivo de `plain` bytes (mesma conta do `FileRef`).
fn enxame_core_enc_len(plain: u64) -> u64 {
    FileRef {
        blob_hash: [0; 32],
        size: plain,
        name: "x".into(),
        mime: String::new(),
        file_key: [0; 32],
    }
    .enc_size()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leaf_serves_its_own_file_but_does_not_seed_what_it_downloaded() {
    // Todos em `Leaf` (padrão de um nó novo): Alice ainda serve o PRÓPRIO arquivo.
    let (a, b, c) = (
        node_with("alice", |_| {}).await,
        node_with("bob", |_| {}).await,
        node_with("carol", |_| {}).await,
    );
    befriend(&a.node, &b.node).await;
    befriend(&a.node, &c.node).await;
    befriend(&b.node, &c.node).await;
    assert_eq!(a.node.role(), enxame_core::Role::Leaf);
    let src = write_file(a.dir.path(), "f.bin", 24 * 1024 * 1024, "f");
    a.node.send_file_to(&["bob", "carol"], &src).await.unwrap();
    c.node
        .download_file("alice", "1", Some(c.dir.path().join("dl")))
        .await
        .unwrap();
    // Carol (Leaf) tem o arquivo mas não semeia: Bob recebe tudo de Alice.
    let a0 = a.node.file_bytes_sent();
    let got = b
        .node
        .download_file("alice", "1", Some(b.dir.path().join("dl")))
        .await
        .unwrap();
    assert!(same_content(&src, &got.path));
    assert_eq!(
        c.node.file_bytes_sent(),
        0,
        "um Leaf não semeia o que baixou"
    );
    assert!(a.node.file_bytes_sent() - a0 > 24 * 1024 * 1024);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exhausted_monthly_budget_stops_seeding_but_the_origin_keeps_serving() {
    let (a, b, c) = (
        peer("alice").await,
        peer("bob").await,
        node_with("carol", |c| {
            c.role = "peer".into();
            c.budget.max_contrib_gb_month = 0; // estourado desde o primeiro byte
        })
        .await,
    );
    befriend(&a.node, &b.node).await;
    befriend(&a.node, &c.node).await;
    befriend(&b.node, &c.node).await;
    let src = write_file(a.dir.path(), "f.bin", 24 * 1024 * 1024, "f");
    a.node.send_file_to(&["bob", "carol"], &src).await.unwrap();
    c.node
        .download_file("alice", "1", Some(c.dir.path().join("dl")))
        .await
        .unwrap();
    let got = b
        .node
        .download_file("alice", "1", Some(b.dir.path().join("dl")))
        .await
        .unwrap();
    assert!(same_content(&src, &got.path));
    assert_eq!(c.node.file_bytes_sent(), 0, "sem budget, Carol não semeia");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_bandwidth_budget_limits_the_served_rate() {
    // 4000 kbps = 500 KB/s. Um arquivo de ~3 MiB leva vários segundos para sair.
    let a = node_with("alice", |c| c.budget.max_contrib_kbps = Some(4000)).await;
    let b = peer("bob").await;
    befriend(&a.node, &b.node).await;
    let src = write_file(a.dir.path(), "lento.bin", 3 * 1024 * 1024, "l");
    a.node.send_file("bob", &src).await.unwrap();
    let t = Instant::now();
    let got = b
        .node
        .download_file("alice", "1", Some(b.dir.path().join("dl")))
        .await
        .unwrap();
    let took = t.elapsed();
    assert!(same_content(&src, &got.path));
    assert!(
        took >= Duration::from_millis(4500),
        "a taxa não foi limitada: {took:?} para ~3 MiB"
    );
    assert!(took < Duration::from_secs(40), "lento demais: {took:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn strangers_and_non_contacts_cannot_fetch_blobs() {
    use iroh::{endpoint::presets, Endpoint, EndpointAddr, RelayMode, TransportAddr};
    let (a, b) = (peer("alice").await, peer("bob").await);
    befriend(&a.node, &b.node).await;
    let src = write_file(a.dir.path(), "secreto.bin", 100_000, "s");
    let sent = a.node.send_file("bob", &src).await.unwrap();

    let stranger = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .unwrap();
    let addr = a.node.endpoint_addr();
    let ips: Vec<_> = addr.ip_addrs().copied().collect();
    let addr = EndpointAddr::from_parts(addr.id, ips.into_iter().map(TransportAddr::Ip));
    let conn = stranger.connect(addr, iroh_blobs::ALPN).await.unwrap();
    let hash = iroh_blobs::Hash::from_bytes(sent.blob);
    // Mesmo conhecendo a raiz (a "capacidade"), quem não é contato mútuo não leva nada.
    let r = tokio::time::timeout(
        Duration::from_secs(15),
        iroh_blobs::get::request::get_blob(conn, hash).bytes_and_stats(),
    )
    .await;
    assert!(
        matches!(r, Ok(Err(_))) || r.is_err(),
        "um estranho conseguiu baixar: {r:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_download_resumes_instead_of_starting_over() {
    let (a, b) = (peer("alice").await, peer("bob").await);
    befriend(&a.node, &b.node).await;
    let src = write_file(a.dir.path(), "retomar.bin", 64 * 1024 * 1024, "r");
    let sent = a.node.send_file("bob", &src).await.unwrap();
    let enc = enxame_core_enc_len(sent.size);
    let dl = b.dir.path().join("dl");

    // Primeira tentativa: corta quando Alice já serviu ~35 %.
    let (node, d) = (b.node.clone(), dl.clone());
    let attempt = tokio::spawn(async move { node.download_file("alice", "1", Some(d)).await });
    let t = Instant::now();
    while a.node.file_bytes_sent() < enc * 35 / 100 {
        assert!(t.elapsed() < Duration::from_secs(60), "download não andou");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    attempt.abort();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let first = a.node.file_bytes_sent();
    assert!(
        !files_of(&b, "alice").await[0].available,
        "ainda incompleto"
    );

    // Segunda tentativa: termina e só pede o que faltava.
    let got = tokio::time::timeout(
        Duration::from_secs(90),
        b.node.download_file("alice", "1", Some(dl)),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(same_content(&src, &got.path));
    let total = a.node.file_bytes_sent();
    assert!(
        total < enc * 160 / 100,
        "reenviou demais: serviu {total} para um arquivo de {enc} (primeira tentativa {first})"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_and_bad_requests_are_refused_cleanly() {
    let (a, b) = (
        node_with("alice", |c| c.max_file_mb = 1).await,
        peer("bob").await,
    );
    assert!(
        a.node
            .send_file("bob", Path::new("/nao/existe"))
            .await
            .is_err(),
        "sem contato"
    );
    befriend(&a.node, &b.node).await;
    // Acima do limite configurado.
    let big = write_file(a.dir.path(), "grande.bin", 2 * 1024 * 1024, "g");
    assert!(a.node.send_file("bob", &big).await.is_err());
    // Diretório e arquivo inexistente.
    assert!(a.node.send_file("bob", a.dir.path()).await.is_err());
    assert!(a
        .node
        .send_file("bob", Path::new("/nao/existe"))
        .await
        .is_err());
    assert!(a.node.send_file_to(&[], &big).await.is_err());
    assert!(a.node.send_file("desconhecido", &big).await.is_err());
    // Nada foi anunciado.
    assert!(files_of(&b, "alice").await.is_empty());
    // Seletores inválidos no download.
    let ok = write_file(a.dir.path(), "ok.txt", 500, "o");
    a.node.send_file("bob", &ok).await.unwrap();
    for bad in ["0", "2", "xx", "", "zzzzzzzz"] {
        assert!(
            b.node.download_file("alice", bad, None).await.is_err(),
            "seletor `{bad}`"
        );
    }
    assert!(b
        .node
        .download_file("alice", "1", Some(b.dir.path().join("dl")))
        .await
        .is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_and_tiny_files_work_and_names_cannot_escape() {
    let (a, b) = (peer("alice").await, peer("bob").await);
    befriend(&a.node, &b.node).await;
    for (name, len) in [("vazio.txt", 0usize), ("um.bin", 1), ("limite.bin", 65_536)] {
        let src = write_file(a.dir.path(), name, len, "");
        a.node.send_file("bob", &src).await.unwrap();
    }
    let dl = b.dir.path().join("dl");
    for (i, name) in ["vazio.txt", "um.bin", "limite.bin"].iter().enumerate() {
        let got = b
            .node
            .download_file("alice", &(i + 1).to_string(), Some(dl.clone()))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(&got.path).unwrap(),
            std::fs::read(a.dir.path().join(name)).unwrap(),
            "{name}"
        );
        assert_eq!(
            got.path.parent().unwrap(),
            dl,
            "ficou dentro do diretório de destino"
        );
    }
}

/// A, B e C mútuos, com os contatos em comum já descobertos (A sabe que C também é contato de B).
async fn trio(a: &TestNode, b: &TestNode, c: &TestNode) {
    befriend(&a.node, &b.node).await;
    befriend(&a.node, &c.node).await;
    befriend(&b.node, &c.node).await;
    a.node.sync_now("bob").await.unwrap();
    a.node.sync_now("carol").await.unwrap();
    b.node.sync_now("carol").await.unwrap();
}

async fn wait_until<F: std::future::Future<Output = bool>>(what: &str, mut f: impl FnMut() -> F) {
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("não ocorreu a tempo: {what}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relay_carrier_replicates_the_file_so_it_arrives_with_the_sender_offline() {
    let a = peer("alice").await;
    let mut b = peer("bob").await;
    let c = node_with("carol", |c| {
        c.role = "relay".into();
        c.blob_gc_interval_secs = 1;
    })
    .await;
    trio(&a, &b, &c).await;

    // Bob cai; Alice manda um arquivo: o envelope e o BLOB ficam com a Carol.
    b.node.shutdown().await;
    let src = write_file(a.dir.path(), "offline.bin", 20 * 1024 * 1024, "o");
    let sent = a.node.send_file("bob", &src).await.unwrap();
    assert!(!sent.messages[0].synced);
    assert_eq!(sent.messages[0].carriers, 1, "{sent:?}");
    wait_until("Carol replicar o blob", || async {
        c.node.has_blob(sent.blob).await
    })
    .await;
    let held = c.node.prefetched_blobs().await.unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].hash, sent.blob);

    // Alice sai de vez. Bob volta, recebe a mensagem pela Carol e baixa o arquivo dela.
    a.node.shutdown().await;
    b.restart().await;
    let got = b.node.carry_now("carol").await.unwrap();
    assert_eq!(got.received, 1, "{got:?}");
    let files = files_of(&b, "alice").await;
    assert_eq!(files.len(), 1);
    assert!(!files[0].available);
    let out = tokio::time::timeout(
        Duration::from_secs(60),
        b.node
            .download_file("alice", "1", Some(b.dir.path().join("dl"))),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(same_content(&src, &out.path), "o arquivo veio diferente");
    // A Carol serviu o arquivo (contribuição dela).
    assert!(c.node.file_bytes_sent() >= sent.size);
    assert!(c.node.contribution_summary().await.unwrap().bytes_served > 0);

    // Vencido o prazo, a cópia some e a coleta de lixo apaga os dados.
    c.node.expire_prefetched_now().await.unwrap();
    assert!(c.node.prefetched_blobs().await.unwrap().is_empty());
    wait_until("a GC apagar o blob", || async {
        !c.node.has_blob(sent.blob).await
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_files_and_weak_carriers_do_not_replicate() {
    // Carol (Relay) limita a replicação a 1 MiB; Dave (Peer) nem aceita mensagens de arquivo.
    for (role, expect_carriers, expect_blob) in [("relay", 1usize, false), ("peer", 0, false)] {
        let a = peer("alice").await;
        let mut b = peer("bob").await;
        let c = node_with("carol", |c| {
            c.role = role.into();
            c.prefetch_max_mb = 1;
        })
        .await;
        trio(&a, &b, &c).await;
        b.node.shutdown().await;
        let src = write_file(a.dir.path(), "grande.bin", 5 * 1024 * 1024, "g");
        let sent = a.node.send_file("bob", &src).await.unwrap();
        assert_eq!(sent.messages[0].carriers, expect_carriers, "papel {role}");
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(
            c.node.has_blob(sent.blob).await,
            expect_blob,
            "papel {role}"
        );
        assert!(
            c.node.prefetched_blobs().await.unwrap().is_empty(),
            "papel {role}"
        );
        // Sem cópia em ninguém e com Alice fora do ar, o arquivo não tem de onde sair.
        if expect_carriers == 1 {
            a.node.shutdown().await;
            b.restart().await;
            b.node.carry_now("carol").await.unwrap();
            let r = b
                .node
                .download_file("alice", "1", Some(b.dir.path().join("dl")))
                .await;
            assert!(r.is_err(), "não deveria haver de onde baixar");
        }
    }
}

/// BLAKE3 de um arquivo, lido em blocos (sem carregar tudo na memória).
fn hash_file(p: &Path) -> String {
    use std::io::Read;
    let mut f = std::fs::File::open(p).unwrap();
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 4 << 20];
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    h.finalize().to_hex().to_string()
}

fn write_big_file(path: &Path, len: u64) {
    use std::io::Write;
    let mut w = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut block = vec![0u8; 1 << 20];
    let mut left = len;
    while left > 0 {
        for chunk in block.chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let b = x.to_le_bytes();
            chunk.copy_from_slice(&b[..chunk.len()]);
        }
        let n = left.min(block.len() as u64) as usize;
        w.write_all(&block[..n]).unwrap();
        left -= n as u64;
    }
    w.flush().unwrap();
}

/// Critério de aceite da Fase 4: arquivo de 1 GiB baixado de 2 fontes simultâneas, com
/// interrupção, retomada e hash verificado. Pesado: rode com
/// `cargo test --release -p enxame-core --test files -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "1 GiB; rode com --release e --ignored"]
async fn one_gib_from_two_sources_with_interruption_and_resume() {
    let size: u64 = 1 << 30;
    let (a, b, c) = (peer("alice").await, peer("bob").await, peer("carol").await);
    befriend(&a.node, &b.node).await;
    befriend(&a.node, &c.node).await;
    befriend(&b.node, &c.node).await;

    let src = a.dir.path().join("um-gigabyte.bin");
    let t = Instant::now();
    write_big_file(&src, size);
    let src_hash = hash_file(&src);
    eprintln!(
        "origem gerada e hasheada em {:?}: blake3 {src_hash}",
        t.elapsed()
    );

    let t = Instant::now();
    let sent = a.node.send_file_to(&["bob", "carol"], &src).await.unwrap();
    let enc = enxame_core_enc_len(sent.size);
    eprintln!(
        "cifrado e importado em {:?} ({} bytes de ciphertext)",
        t.elapsed(),
        enc
    );

    // Carol baixa de Alice e passa a ser a segunda fonte.
    let t = Instant::now();
    let cgot = c
        .node
        .download_file("alice", "1", Some(c.dir.path().join("dl")))
        .await
        .unwrap();
    eprintln!("Carol baixou de Alice em {:?}", t.elapsed());
    assert_eq!(hash_file(&cgot.path), src_hash, "arquivo da Carol difere");

    // Bob baixa das duas; interrompe com ~30 % e retoma.
    let (a0, c0) = (a.node.file_bytes_sent(), c.node.file_bytes_sent());
    let dl = b.dir.path().join("dl");
    let (node, d) = (b.node.clone(), dl.clone());
    let t = Instant::now();
    let attempt = tokio::spawn(async move { node.download_file("alice", "1", Some(d)).await });
    while (a.node.file_bytes_sent() - a0) + (c.node.file_bytes_sent() - c0) < enc * 30 / 100 {
        assert!(
            t.elapsed() < Duration::from_secs(600),
            "o download não andou"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    attempt.abort();
    tokio::time::sleep(Duration::from_secs(1)).await;
    eprintln!(
        "interrompido em {:?} com {} bytes servidos",
        t.elapsed(),
        (a.node.file_bytes_sent() - a0) + (c.node.file_bytes_sent() - c0)
    );
    assert!(!files_of(&b, "alice").await[0].available);

    let t2 = Instant::now();
    let bgot = b.node.download_file("alice", "1", Some(dl)).await.unwrap();
    eprintln!("retomado e concluído em {:?}", t2.elapsed());
    let (from_a, from_c) = (a.node.file_bytes_sent() - a0, c.node.file_bytes_sent() - c0);
    eprintln!("Bob recebeu {from_a} bytes de Alice e {from_c} de Carol (arquivo cifrado: {enc})");
    assert_eq!(hash_file(&bgot.path), src_hash, "arquivo do Bob difere");
    assert!(
        from_a > enc / 4 && from_c > enc / 4,
        "as duas fontes deveriam servir bastante: {from_a} / {from_c}"
    );
    assert!(
        from_a + from_c < enc * 15 / 10,
        "reenviou demais: {} de {enc}",
        from_a + from_c
    );
}
