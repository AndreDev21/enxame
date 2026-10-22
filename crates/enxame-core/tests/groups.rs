#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Grupos pequenos (Fase 5, ADR 0009): nós in-process, sem relay e sem rede externa.

use std::{path::Path, time::Duration};

use enxame_core::{AddOutcome, Config, Enxame, Event, FileKeyStore, MAX_GROUP_MEMBERS};
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
    /// Reinicia o nó com o mesmo diretório (mesma identidade e banco; porta nova).
    async fn restart(&mut self) {
        self.node.shutdown().await;
        self.node = start_at(self.dir.path(), &self.name).await;
    }

    async fn texts(&self, group: &str) -> Vec<String> {
        self.node
            .group_history(group, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.text)
            .collect()
    }
}

/// `b` adiciona `a`; `a` aceita. Ao final são contatos mútuos.
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

/// Espera `f` (com tempo limite) e falha com `what`.
async fn eventually<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("não aconteceu a tempo: {what}");
}

/// Criador `a` com contatos mútuos `others`; os `others` não se conhecem entre si.
async fn star(creator: &TestNode, others: &[&TestNode]) {
    for o in others {
        befriend(&creator.node, &o.node).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn five_nodes_create_add_message_remove_and_the_removed_cannot_read_new_messages() {
    let (a, b, c, d, e) = (
        node("ana").await,
        node("beto").await,
        node("cris").await,
        node("dani").await,
        node("edu").await,
    );
    star(&a, &[&b, &c, &d, &e]).await;

    // B e C não são contatos: só o criador conhece todos.
    let mut b_events = b.node.subscribe();
    let (info, direct) = a
        .node
        .create_group("amigos", &["beto", "cris", "dani"])
        .await
        .unwrap();
    assert_eq!((info.members.len(), direct), (4, 3), "{info:?}");
    assert!(info.i_am_creator && info.active);

    // Cada membro recebe o retrato, e o aviso de IP lista quem não é contato dele.
    for n in [&b, &c, &d] {
        eventually("grupo chegou", || async {
            n.node.groups().await.unwrap().iter().any(|g| g.active)
        })
        .await;
    }
    let joined = loop {
        match tokio::time::timeout(Duration::from_secs(5), b_events.recv())
            .await
            .unwrap()
            .unwrap()
        {
            Event::GroupJoined {
                name,
                creator_name,
                exposed,
                ..
            } => break (name, creator_name, exposed),
            _ => continue,
        }
    };
    assert_eq!(joined.0, "amigos");
    assert_eq!(joined.1, "ana");
    let mut exposed = joined.2;
    exposed.sort();
    assert_eq!(exposed.len(), 2, "cris e dani expostos a beto: {exposed:?}");
    assert!(exposed[0].starts_with("cris") && exposed[1].starts_with("dani"));

    // Mensagem de B (que não é contato de C nem de D) chega aos três.
    let sent = b.node.send_group_text("amigos", "oi, grupo").await.unwrap();
    assert_eq!((sent.delivered, sent.offline.len()), (3, 0), "{sent:?}");
    for n in [&a, &c, &d] {
        assert_eq!(n.texts("amigos").await, vec!["oi, grupo"], "{}", n.name);
    }
    let hist = b.node.group_history("amigos", 10).await.unwrap();
    assert_eq!(hist[0].delivered, Some(true));
    assert_eq!(hist[0].author_name, "beto");

    // O criador adiciona E: E lê o que vier depois, não o que veio antes.
    let (info, _) = a.node.add_group_member("amigos", "edu").await.unwrap();
    assert_eq!(info.members.len(), 5);
    eventually("E entrou", || async {
        e.node.groups().await.unwrap().iter().any(|g| g.active)
    })
    .await;
    for n in [&b, &c, &d] {
        eventually("retrato novo", || async {
            n.node.groups().await.unwrap()[0].members.len() == 5
        })
        .await;
    }
    e.node.sync_group("amigos").await.unwrap();
    assert!(
        e.texts("amigos").await.is_empty(),
        "histórico anterior é ilegível para E"
    );
    c.node
        .send_group_text("amigos", "bem-vindo, edu")
        .await
        .unwrap();
    assert_eq!(e.texts("amigos").await, vec!["bem-vindo, edu"]);

    // Só o criador mexe na lista.
    let err = b.node.add_group_member("amigos", "ana").await.unwrap_err();
    assert!(err.to_string().contains("criador"), "{err}");
    let err = a
        .node
        .remove_group_member("amigos", "ana")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("criador"), "{err}");

    // Remoção de D: D fica sabendo, e não decifra nada novo.
    let mut d_events = d.node.subscribe();
    a.node.remove_group_member("amigos", "dani").await.unwrap();
    eventually("D soube", || async {
        d.node.groups().await.unwrap().iter().all(|g| !g.active)
    })
    .await;
    assert!(matches!(
        loop {
            match tokio::time::timeout(Duration::from_secs(5), d_events.recv())
                .await
                .unwrap()
                .unwrap()
            {
                ev @ Event::GroupRemoved { .. } => break ev,
                _ => continue,
            }
        },
        Event::GroupRemoved { .. }
    ));
    for n in [&b, &c, &e] {
        eventually("retrato sem D", || async {
            n.node.groups().await.unwrap()[0].members.len() == 4
        })
        .await;
    }
    let sent = b
        .node
        .send_group_text("amigos", "sem o dani")
        .await
        .unwrap();
    assert_eq!(sent.delivered, 3, "{sent:?}");
    // D tenta sincronizar com os ex-colegas: nada novo, e nada que ele consiga ler.
    let _ = d.node.sync_group("amigos").await;
    let d_texts = d.texts("amigos").await;
    assert!(!d_texts.contains(&"sem o dani".to_string()), "{d_texts:?}");
    assert!(d
        .node
        .send_group_text("amigos", "ainda aqui?")
        .await
        .is_err());
    // Os demais continuam conversando.
    assert_eq!(e.texts("amigos").await.last().unwrap(), "sem o dani");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_catches_up_from_another_member_while_the_author_is_offline() {
    let (a, b, c, mut d) = (
        node("ana").await,
        node("beto").await,
        node("cris").await,
        node("dani").await,
    );
    star(&a, &[&b, &c, &d]).await;
    a.node
        .create_group("amigos", &["beto", "cris", "dani"])
        .await
        .unwrap();
    for n in [&b, &c, &d] {
        eventually("grupo chegou", || async {
            n.node.groups().await.unwrap().iter().any(|g| g.active)
        })
        .await;
    }

    // D cai. A escreve: B e C recebem, D fica pendente.
    d.node.shutdown().await;
    let sent = a
        .node
        .send_group_text("amigos", "escrevi com o dani fora")
        .await
        .unwrap();
    assert_eq!(sent.delivered, 2);
    assert_eq!(sent.offline, vec!["dani".to_string()]);
    let hist = a.node.group_history("amigos", 5).await.unwrap();
    assert_eq!(hist[0].delivered, Some(false), "ainda falta o dani");

    // A também some: só B, C e D se encontram. D volta (porta nova) e puxa de quem tiver.
    a.node.shutdown().await;
    d.restart().await;
    let reached = d.node.sync_group("amigos").await.unwrap();
    assert!(
        reached >= 2,
        "D alcança B e C, que não são seus contatos: {reached}"
    );
    assert_eq!(d.texts("amigos").await, vec!["escrevi com o dani fora"]);
    assert!(b.node.groups().await.unwrap()[0]
        .members
        .iter()
        .any(|m| !m.is_contact));
}

#[tokio::test(flavor = "multi_thread")]
async fn group_limits_and_validation() {
    let a = node("ana").await;
    let b = node("beto").await;
    befriend(&a.node, &b.node).await;
    // Sem contatos não há grupo; contato inexistente ou nome vazio também falham.
    assert!(a.node.create_group("g", &[]).await.is_err());
    assert!(a.node.create_group("g", &["ninguem"]).await.is_err());
    assert!(a.node.create_group("  ", &["beto"]).await.is_err());
    assert!(a.node.send_group_text("nao-existe", "oi").await.is_err());
    let (info, _) = a.node.create_group("g", &["beto"]).await.unwrap();
    assert_eq!(info.members.len(), 2);
    assert!(a.node.send_group_text("#g", "   ").await.is_err());
    assert!(
        a.node.add_group_member("g", "beto").await.is_err(),
        "já está"
    );
    // Renomear mantém o mesmo grupo.
    let (renamed, _) = a.node.rename_group("g", "novo nome").await.unwrap();
    assert_eq!((renamed.id, renamed.name.as_str()), (info.id, "novo nome"));
    assert_eq!(MAX_GROUP_MEMBERS, 20);
}

/// Aceite da Fase 5: carry entre membros. B (membro, não é contato de D) carrega a mensagem de A
/// para D enquanto A está offline; o recibo de D chega de volta a B.
#[tokio::test(flavor = "multi_thread")]
async fn a_member_who_is_not_the_recipients_contact_carries_for_the_group() {
    let dir = |n: &str| {
        let d = tempfile::tempdir().unwrap();
        (d, n.to_owned())
    };
    let peer_node = |name: &'static str| async move {
        let (d, n) = dir(name);
        let mut cfg = Config::for_tests(d.path().to_path_buf(), &n);
        cfg.role = "peer".into();
        let store = FileKeyStore::new(d.path().join("identity.key"));
        let node = Enxame::start(cfg, &store).await.unwrap();
        TestNode {
            node,
            dir: d,
            name: n,
        }
    };
    let (a, b, c, mut d) = (
        peer_node("ana").await,
        peer_node("beto").await,
        peer_node("cris").await,
        peer_node("dani").await,
    );
    star(&a, &[&b, &c, &d]).await;
    a.node
        .create_group("amigos", &["beto", "cris", "dani"])
        .await
        .unwrap();
    for n in [&b, &c, &d] {
        eventually("grupo chegou", || async {
            n.node.groups().await.unwrap().iter().any(|g| g.active)
        })
        .await;
    }
    // Todos se conhecem como pares (conversa de grupo) antes de D cair: B e C sincronizam com D.
    for n in [&a, &b, &c] {
        n.node.sync_group("amigos").await.unwrap();
    }

    // D cai e A escreve: B e C recebem e A pede a eles que carreguem para D.
    d.node.shutdown().await;
    let sent = a
        .node
        .send_group_text("amigos", "carregue pra mim")
        .await
        .unwrap();
    assert_eq!(sent.offline, vec!["dani".to_string()], "{sent:?}");
    assert!(sent.carriers >= 1, "B ou C aceitaram carregar: {sent:?}");
    a.node.shutdown().await;

    // D volta e encontra B (que não é seu contato) pelo carry; a mensagem é entregue.
    d.restart().await;
    let got = tokio::time::timeout(Duration::from_secs(20), d.node.carry_now("beto"))
        .await
        .unwrap()
        .unwrap();
    assert!(got.received >= 1, "{got:?}");
    assert_eq!(d.texts("amigos").await, vec!["carregue pra mim"]);
    // O recibo de D esvaziou a fila de B.
    let sum = b.node.contribution_summary().await.unwrap();
    assert_eq!(sum.carry_items, 0, "{sum:?}");
}
