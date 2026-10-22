#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integração das mensagens 1:1: nós in-process, sem relay e sem rede externa.

use std::{path::Path, time::Duration};

use enxame_core::{AddOutcome, Config, Enxame, Event, FileKeyStore};
use tempfile::TempDir;

struct TestNode {
    node: Enxame,
    dir: TempDir,
    name: String,
}

async fn start_at(dir: &Path, name: &str) -> Enxame {
    let cfg = Config::for_tests(dir.to_path_buf(), name);
    let store = FileKeyStore::new(dir.join("identity.key"));
    Enxame::start(cfg, &store).await.unwrap()
}

async fn node(name: &str) -> TestNode {
    let dir = tempfile::tempdir().unwrap();
    let node = start_at(dir.path(), name).await;
    TestNode {
        node,
        dir,
        name: name.into(),
    }
}

impl TestNode {
    /// Reinicia o nó com o mesmo diretório (mesma identidade e banco).
    async fn restart(&mut self) {
        self.node.shutdown().await;
        self.node = start_at(self.dir.path(), &self.name).await;
    }

    async fn texts(&self, with: &str) -> Vec<String> {
        self.node
            .history(with, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.text)
            .collect()
    }
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

#[tokio::test(flavor = "multi_thread")]
async fn messages_flow_both_ways_while_online() {
    let (a, b) = (node("alice").await, node("bob").await);
    befriend(&a.node, &b.node).await;

    let mut events = b.node.subscribe();
    let sent = a.node.send_text("bob", "oi, bob").await.unwrap();
    assert!(sent.synced, "{sent:?}");
    // B recebeu e avisou pelo stream de eventos.
    let ev = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(Event::MessageReceived { from_name, .. }) = events.recv().await {
                return from_name;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(ev, "alice");
    assert_eq!(b.texts("alice").await, vec!["oi, bob"]);

    let sent = b.node.send_text("alice", "oi, alice").await.unwrap();
    assert!(sent.synced, "{sent:?}");
    assert_eq!(a.texts("bob").await, vec!["oi, bob", "oi, alice"]);
    assert_eq!(b.texts("alice").await, vec!["oi, bob", "oi, alice"]);

    let hist = a.node.history("bob", 10).await.unwrap();
    assert!(hist[0].from_me && !hist[1].from_me);
    assert_eq!(hist[1].author_name, "bob");
}

#[tokio::test(flavor = "multi_thread")]
async fn offline_messages_persist_and_sync_after_reconnect() {
    let (a, mut b) = (node("alice").await, node("bob").await);
    befriend(&a.node, &b.node).await;
    a.node.send_text("bob", "antes").await.unwrap();

    // Bob cai. Alice escreve: fica guardado, não entregue.
    b.node.shutdown().await;
    let s1 = a.node.send_text("bob", "durante 1").await.unwrap();
    let s2 = a.node.send_text("bob", "durante 2").await.unwrap();
    assert!(!s1.synced && !s2.synced);
    assert!(s1.error.is_some());
    assert_eq!(
        a.texts("bob").await,
        vec!["antes", "durante 1", "durante 2"]
    );

    // Bob volta e reconecta (porta nova, então é ele quem disca): recebe o que perdeu.
    b.node = start_at(b.dir.path(), "bob").await;
    let stats = tokio::time::timeout(Duration::from_secs(20), b.node.sync_now("alice"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stats.received, 2, "{stats:?}");
    assert_eq!(
        b.texts("alice").await,
        vec!["antes", "durante 1", "durante 2"]
    );

    // E a ordem causal se mantém quando Bob responde e os dois ficam em dia.
    b.node.send_text("alice", "voltei").await.unwrap();
    assert_eq!(
        a.texts("bob").await,
        vec!["antes", "durante 1", "durante 2", "voltei"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn history_and_pending_messages_survive_a_restart() {
    let (mut a, b) = (node("alice").await, node("bob").await);
    befriend(&a.node, &b.node).await;
    a.node.send_text("bob", "entregue").await.unwrap();
    b.node.shutdown().await;
    let pending = a.node.send_text("bob", "pendente").await.unwrap();
    assert!(!pending.synced);
    a.restart().await;
    assert_eq!(a.texts("bob").await, vec!["entregue", "pendente"]);
    // O relógio retoma depois do maior HLC gravado: a próxima mensagem vem depois das antigas.
    let before = a.node.history("bob", 10).await.unwrap();
    a.node.send_text("bob", "pós-reinício").await.unwrap();
    let after = a.node.history("bob", 10).await.unwrap();
    assert_eq!(after.len(), 3);
    assert!(after[2].hlc > before[1].hlc);
}

#[tokio::test(flavor = "multi_thread")]
async fn conversations_are_isolated_between_three_nodes() {
    let (a, b, c) = (node("alice").await, node("bob").await, node("carol").await);
    befriend(&a.node, &b.node).await;
    befriend(&a.node, &c.node).await;

    a.node.send_text("bob", "só pro bob").await.unwrap();
    a.node.send_text("carol", "só pra carol").await.unwrap();
    c.node
        .send_text("alice", "resposta da carol")
        .await
        .unwrap();

    assert_eq!(b.texts("alice").await, vec!["só pro bob"]);
    assert_eq!(
        c.texts("alice").await,
        vec!["só pra carol", "resposta da carol"]
    );
    assert_eq!(a.texts("bob").await, vec!["só pro bob"]);
    assert_eq!(
        a.texts("carol").await,
        vec!["só pra carol", "resposta da carol"]
    );
    // Bob e Carol não são contatos entre si: nem conseguem consultar nem enviar.
    assert!(b.node.history("carol", 10).await.is_err());
    assert!(b.node.send_text("carol", "oi").await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn sending_validates_input_and_contact_state() {
    let (a, b) = (node("alice").await, node("bob").await);
    assert!(a.node.send_text("bob", "oi").await.is_err(), "sem contato");
    befriend(&a.node, &b.node).await;
    assert!(a.node.send_text("bob", "   ").await.is_err(), "vazia");
    let huge = "x".repeat(17 * 1024);
    assert!(
        a.node.send_text("bob", &huge).await.is_err(),
        "grande demais"
    );
    assert!(a.node.history("bob", 10).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn phase1_contacts_file_is_migrated_to_the_database() {
    // Cria um diretório de dados "da Fase 1": contatos em contacts.bin, sem banco.
    let (a, b) = (node("alice").await, node("bob").await);
    befriend(&a.node, &b.node).await;
    let legacy = enxame_core::testing::legacy_contacts_bytes(&a.node);
    a.node.shutdown().await;
    let _ = std::fs::remove_file(a.dir.path().join("enxame.db"));
    let _ = std::fs::remove_file(a.dir.path().join("enxame.db-wal"));
    let _ = std::fs::remove_file(a.dir.path().join("enxame.db-shm"));
    std::fs::write(a.dir.path().join("contacts.bin"), legacy).unwrap();

    let a2 = start_at(a.dir.path(), "alice").await;
    let contacts = a2.contacts();
    assert_eq!(contacts.len(), 1);
    assert!(contacts[0].is_mutual());
    assert!(a.dir.path().join("contacts.bin.migrated").exists());
    assert!(!a.dir.path().join("contacts.bin").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn read_history_reads_the_database_while_the_node_is_running() {
    let (a, b) = (node("alice").await, node("bob").await);
    befriend(&a.node, &b.node).await;
    a.node.send_text("bob", "primeira").await.unwrap();
    a.node.send_text("bob", "segunda").await.unwrap();

    // Sem subir outro endpoint: o nó `a` continua com a mesma chave no ar.
    let cfg = Config::for_tests(a.dir.path().to_path_buf(), "alice");
    let store = FileKeyStore::new(a.dir.path().join("identity.key"));
    let read = enxame_core::read_history(&cfg, &store, "bob", 10)
        .await
        .unwrap();
    let texts: Vec<_> = read.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["primeira", "segunda"]);
    assert_eq!(texts, a.texts("bob").await);
    assert!(read.iter().all(|m| m.from_me));
}
