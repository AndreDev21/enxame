#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Sinalização de chamadas (Fase 6.4, ADR 0012): nós in-process, sem relay.

use std::{path::Path, time::Duration};

use enxame_core::{AddOutcome, Config, Enxame, Event, FileKeyStore};
use tempfile::TempDir;

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

async fn next_signal(events: &mut tokio::sync::broadcast::Receiver<Event>) -> (String, Vec<u8>) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(Event::CallSignal {
                from_name, payload, ..
            }) = events.recv().await
            {
                return (from_name, payload);
            }
        }
    })
    .await
    .expect("o sinal deveria chegar")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_signal_reaches_the_contact_in_real_time_and_leaves_no_trace() {
    let ((a, _da), (b, _db)) = (node("alice").await, node("bob").await);
    befriend(&a, &b).await;
    let mut events = b.subscribe();

    a.send_call_signal("bob", br#"{"t":"offer","sdp":"v=0"}"#)
        .await
        .unwrap();
    let (from, payload) = next_signal(&mut events).await;
    assert_eq!(
        (from.as_str(), payload.as_slice()),
        ("alice", &br#"{"t":"offer","sdp":"v=0"}"#[..])
    );

    // Vários sinais seguidos (candidatos ICE) chegam todos e na ordem de envio.
    for i in 0..10 {
        a.send_call_signal("bob", format!("ice-{i}").as_bytes())
            .await
            .unwrap();
    }
    for i in 0..10 {
        assert_eq!(
            next_signal(&mut events).await.1,
            format!("ice-{i}").into_bytes()
        );
    }
    // Sem histórico nem pendência: é tempo real.
    assert!(b.history("alice", 10).await.unwrap().is_empty());
    assert!(a.history("bob", 10).await.unwrap().is_empty());
    // Sinal vazio e grande demais são recusados na origem.
    assert!(a.send_call_signal("bob", b"").await.is_err());
    assert!(a
        .send_call_signal("bob", &vec![0u8; 70 * 1024])
        .await
        .is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn strangers_and_offline_contacts_cannot_be_signaled() {
    let ((a, _da), (b, _db), (c, _dc)) =
        (node("alice").await, node("bob").await, node("carol").await);
    befriend(&a, &b).await;
    // Carol não é contato de ninguém: nem consegue endereçar (não há contato) ...
    assert!(c.send_call_signal("alice", b"x").await.is_err());
    // ... e a Alice não sinaliza quem não é mútuo.
    assert!(a.send_call_signal("carol", b"x").await.is_err());

    // Bob cai: a chamada falha logo (nada fica na fila).
    b.shutdown().await;
    let started = std::time::Instant::now();
    assert!(a.send_call_signal("bob", b"x").await.is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
}
