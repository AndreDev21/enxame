#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integração da Fase 3: carry (cenário A, B, C), recibos, papéis e budget. Nós in-process,
//! sem relay e sem rede externa.

use std::{path::Path, sync::Arc, time::Duration};

use enxame_core::{
    AddOutcome, Config, Enxame, Event, FileKeyStore, LinkKind, PlatformReading, Role, StartOptions,
    StaticSignals,
};
use tempfile::TempDir;

struct TestNode {
    node: Enxame,
    _dir: TempDir,
    cfg: Config,
}

fn base_cfg(dir: &Path, name: &str) -> Config {
    Config::for_tests(dir.to_path_buf(), name)
}

async fn start_cfg(cfg: &Config, opts: StartOptions) -> Enxame {
    let store = FileKeyStore::new(cfg.data_dir.join("identity.key"));
    Enxame::start_with_options(cfg.clone(), &store, opts)
        .await
        .unwrap()
}

async fn node_with(name: &str, tweak: impl FnOnce(&mut Config)) -> TestNode {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = base_cfg(dir.path(), name);
    tweak(&mut cfg);
    let node = start_cfg(&cfg, StartOptions::default()).await;
    TestNode {
        node,
        _dir: dir,
        cfg,
    }
}

async fn node(name: &str) -> TestNode {
    node_with(name, |_| {}).await
}

impl TestNode {
    /// Reinicia com a mesma identidade e o mesmo banco (porta nova).
    async fn restart(&mut self) {
        self.node.shutdown().await;
        self.node = start_cfg(&self.cfg, StartOptions::default()).await;
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

/// A, B e C, todos contatos mútuos entre si, e cada par já sincronizado uma vez para trocar a
/// lista de contatos em comum.
async fn trio(a: &TestNode, b: &TestNode, c: &TestNode) {
    befriend(&a.node, &b.node).await;
    befriend(&a.node, &c.node).await;
    befriend(&b.node, &c.node).await;
    for (x, y) in [(&a.node, "bob"), (&a.node, "carol")] {
        x.sync_now(y).await.unwrap();
    }
    b.node.sync_now("carol").await.unwrap();
}

async fn wait_for<F: Fn() -> bool>(f: F) {
    for _ in 0..100 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("condição não atendida a tempo");
}

/// O cenário da spec 13 (Fase 3): B offline, A envia e fica offline, C online; B volta e recebe
/// via C; o recibo remove a cópia em C.
async fn offline_delivery_through_c(c_tweak: impl FnOnce(&mut Config)) {
    let (mut a, mut b, c) = (
        node("alice").await,
        node("bob").await,
        node_with("carol", c_tweak).await,
    );
    trio(&a, &b, &c).await;

    // B cai. A escreve: sem como falar com B, oferece aos contatos em comum.
    b.node.shutdown().await;
    let sent = a
        .node
        .send_text("bob", "olá bob, escrevi com você fora")
        .await
        .unwrap();
    assert!(!sent.synced);
    assert_eq!(sent.carriers, 1, "C aceitou carregar: {sent:?}");
    let hist = a.node.history("bob", 10).await.unwrap();
    assert_eq!(hist[0].delivered, Some(false), "ainda não entregue");
    assert_eq!(c.node.contribution_summary().await.unwrap().carry_items, 1);

    // A também cai (só C e B vão se encontrar).
    a.node.shutdown().await;

    // B volta (porta nova) e encontra C: recebe a mensagem escrita por A.
    b.restart().await;
    let got = tokio::time::timeout(Duration::from_secs(20), b.node.carry_now("carol"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.received, 1, "{got:?}");
    assert_eq!(
        b.texts("alice").await,
        vec!["olá bob, escrevi com você fora"]
    );

    // O recibo de B voltou a C na mesma sessão e a cópia foi expurgada.
    let summary = c.node.contribution_summary().await.unwrap();
    assert_eq!(summary.carry_items, 0, "cópia em C removida: {summary:?}");
    assert_eq!(summary.items_delivered, 1);
    assert!(summary.bytes_served > 0);
    assert_eq!(summary.by_contact[0].0, "bob");

    // Uma segunda sessão não entrega nem duplica nada.
    let again = b.node.carry_now("carol").await.unwrap();
    assert_eq!(again.received, 0);
    assert_eq!(b.texts("alice").await.len(), 1);

    // A volta (porta nova) e encontra C: recebe o recibo de B e para de tratar como pendente.
    a.restart().await;
    let got = tokio::time::timeout(Duration::from_secs(20), a.node.carry_now("carol"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.receipts_learned, 1, "{got:?}");
    let hist = a.node.history("bob", 10).await.unwrap();
    assert_eq!(hist[0].delivered, Some(true), "entregue (por recibo)");
}

#[tokio::test(flavor = "multi_thread")]
async fn b_offline_a_sends_and_leaves_c_carries_b_returns_and_receipt_purges_the_copy() {
    // C é um contato comum já promovido a `Peer` (um nó recém-aberto começa em `Leaf`, que não
    // carrega, e só sobe depois de a condição se manter estável).
    offline_delivery_through_c(|cfg| cfg.role = "peer".into()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn same_scenario_with_a_headless_super_peer_node_as_carrier() {
    // O que o `enxame-node` configura: papel fixo SuperPeer, contribuição ligada.
    offline_delivery_through_c(|cfg| {
        cfg.role = "super_peer".into();
        cfg.auto_accept_contacts = true;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leaf_does_not_carry_so_the_message_waits_for_the_direct_sync() {
    let (a, mut b, c) = (
        node("alice").await,
        node("bob").await,
        node_with("carol", |c| c.role = "leaf".into()).await,
    );
    trio(&a, &b, &c).await;
    b.node.shutdown().await;
    let sent = a.node.send_text("bob", "ninguém carrega").await.unwrap();
    assert_eq!((sent.synced, sent.carriers), (false, 0));
    assert_eq!(c.node.contribution_summary().await.unwrap().carry_items, 0);
    // B volta e fala direto com A: a pendência resolve pelo caminho normal.
    b.restart().await;
    let stats = tokio::time::timeout(Duration::from_secs(20), b.node.sync_now("alice"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stats.received, 1);
    assert_eq!(b.texts("alice").await, vec!["ninguém carrega"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_best_carriers_are_chosen_and_at_most_three_copies_exist() {
    // A tem 5 contatos em comum com B; só 3 devem guardar cópia, e Leaf nunca é escolhido.
    let a = node("alice").await;
    let mut b = node("bob").await;
    befriend(&a.node, &b.node).await;
    let roles = ["leaf", "leaf_plus", "peer", "relay", "super_peer"];
    let mut carriers = Vec::new();
    for (i, role) in roles.iter().enumerate() {
        let name = format!("c{i}-{role}");
        let c = node_with(&name, |cfg| cfg.role = (*role).into()).await;
        befriend(&a.node, &c.node).await;
        befriend(&b.node, &c.node).await;
        carriers.push((role, c));
    }
    // Troca de papéis conhecidos (Hello) e de contatos em comum.
    a.node.sync_now("bob").await.unwrap();
    for (_, c) in &carriers {
        a.node.sync_now(c.node.display_name()).await.unwrap();
    }
    b.node.shutdown().await;
    let sent = a.node.send_text("bob", "para três carriers").await.unwrap();
    assert_eq!(sent.carriers, 3, "{sent:?}");
    let mut holding = Vec::new();
    for (role, c) in &carriers {
        if c.node.contribution_summary().await.unwrap().carry_items == 1 {
            holding.push(**role);
        }
    }
    holding.sort();
    assert_eq!(
        holding,
        vec!["peer", "relay", "super_peer"],
        "prioridade SuperPeer > Relay > Peer"
    );
    b.restart().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn carry_quota_is_enforced_per_role() {
    // LeafPlus com quota de 0 MB não guarda nada: a oferta é recusada por quota.
    let (a, mut b, c) = (
        node("alice").await,
        node("bob").await,
        node_with("carol", |c| {
            c.role = "leaf_plus".into();
            c.budget.carry_quota_mb_total = Some(0);
            c.budget.carry_quota_mb_per_contact = Some(0);
        })
        .await,
    );
    trio(&a, &b, &c).await;
    b.node.shutdown().await;
    let sent = a.node.send_text("bob", "não cabe").await.unwrap();
    assert_eq!(sent.carriers, 0);
    assert_eq!(c.node.contribution_summary().await.unwrap().carry_items, 0);
    b.restart().await;
}

fn reading(link: LinkKind) -> PlatformReading {
    PlatformReading {
        on_ac_power: true,
        battery_pct: None,
        metered: Some(false),
        link_kind: link,
        thermal_ok: true,
        cpu_free: Some(0.9),
        mem_free_mb: Some(4096),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn roles_follow_signals_with_hysteresis_on_a_mocked_clock() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = base_cfg(dir.path(), "alice");
    let signals = Arc::new(StaticSignals::new(reading(LinkKind::Ethernet)));
    let node = start_cfg(
        &cfg,
        StartOptions {
            signals: Some(signals.clone()),
        },
    )
    .await;
    let mut events = node.subscribe();
    let s = Duration::from_secs;

    assert_eq!(node.role(), Role::Leaf, "começa conservador");
    assert_eq!(node.tick_roles_at(s(0)).await, None);
    assert_eq!(node.tick_roles_at(s(119)).await, None, "promoção atrasada");
    assert_eq!(
        node.tick_roles_at(s(120)).await,
        Some((Role::Leaf, Role::Relay))
    );
    assert_eq!(node.role(), Role::Relay);
    assert!(matches!(
        events.try_recv(),
        Ok(Event::RoleChanged {
            from: Role::Leaf,
            to: Role::Relay
        })
    ));

    // Passa a usar dados móveis: rebaixa na hora, sem esperar.
    signals.set(reading(LinkKind::Cellular));
    assert_eq!(
        node.tick_roles_at(s(121)).await,
        Some((Role::Relay, Role::LeafPlus))
    );

    // Volta ao cabo: sobe de novo só depois de 120 s estável.
    signals.set(reading(LinkKind::Ethernet));
    assert_eq!(node.tick_roles_at(s(200)).await, None);
    assert_eq!(node.tick_roles_at(s(319)).await, None);
    assert_eq!(
        node.tick_roles_at(s(320)).await,
        Some((Role::LeafPlus, Role::Relay))
    );

    // Modo jogo: Leaf imediatamente; ao desligar, a promoção volta a esperar.
    assert_eq!(
        node.set_game_mode(true).await,
        Some((Role::Relay, Role::Leaf))
    );
    assert_eq!(node.set_contribution(false).await, None, "já é Leaf");
    node.set_game_mode(false).await;
    assert_eq!(node.role(), Role::Leaf, "contribuição ainda desligada");
    node.set_contribution(true).await;
    assert_eq!(
        node.role(),
        Role::Leaf,
        "promoção ainda precisa de tempo estável"
    );
    node.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn budget_over_the_monthly_cap_demotes_to_leaf_plus() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = base_cfg(dir.path(), "alice");
    cfg.promote_after_secs = 0;
    cfg.promote_after_super_secs = 0;
    cfg.budget.max_contrib_gb_month = 0; // qualquer contribuição já estoura
    let signals = Arc::new(StaticSignals::new(reading(LinkKind::Ethernet)));
    let node = start_cfg(
        &cfg,
        StartOptions {
            signals: Some(signals),
        },
    )
    .await;
    // Sem nada servido ainda, 0 GB de teto já conta como estourado.
    assert_eq!(
        node.tick_roles_at(Duration::from_secs(1)).await,
        Some((Role::Leaf, Role::LeafPlus))
    );
    node.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn game_mode_node_refuses_carry() {
    let (a, mut b) = (node("alice").await, node("bob").await);
    let dir = tempfile::tempdir().unwrap();
    let mut ccfg = base_cfg(dir.path(), "carol");
    ccfg.promote_after_secs = 0;
    ccfg.promote_after_super_secs = 0;
    let signals = Arc::new(StaticSignals::new(reading(LinkKind::Ethernet)));
    let c = start_cfg(
        &ccfg,
        StartOptions {
            signals: Some(signals),
        },
    )
    .await;
    let c = TestNode {
        node: c,
        _dir: dir,
        cfg: ccfg,
    };
    trio(&a, &b, &c).await;
    // C tem sinais bons e vira carrier... a menos que entre em modo jogo.
    c.node.tick_roles_at(Duration::from_secs(1)).await;
    assert!(c.node.role() >= Role::Relay);
    c.node.set_game_mode(true).await;
    assert_eq!(c.node.role(), Role::Leaf);
    b.node.shutdown().await;
    let sent = a.node.send_text("bob", "C está jogando").await.unwrap();
    assert_eq!(sent.carriers, 0);
    b.restart().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn contact_policy_allows_listed_fingerprints_and_rejects_nobody_by_default() {
    let a = node("alice").await;
    let b_dir = tempfile::tempdir().unwrap();
    let b_cfg = base_cfg(b_dir.path(), "bob");
    let b = start_cfg(&b_cfg, StartOptions::default()).await;
    let fp = enxame_core::fingerprint(&b.identity_pk());
    // Nó headless: só aceita a fingerprint listada, sem perguntar a ninguém.
    let node_dir = tempfile::tempdir().unwrap();
    let mut ncfg = base_cfg(node_dir.path(), "node");
    ncfg.role = "super_peer".into();
    ncfg.allowed_fingerprints = vec![fp.to_uppercase()];
    let headless = start_cfg(&ncfg, StartOptions::default()).await;
    let ticket = headless.invite_ticket().await.unwrap().to_string();
    let out = tokio::time::timeout(Duration::from_secs(10), b.add_contact(&ticket))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(out, AddOutcome::Accepted(_)), "{out:?}");
    assert_eq!(headless.role(), Role::SuperPeer, "papel fixo");
    // Quem não está na lista cai no fluxo normal (fica pendente, sem aceitar sozinho).
    let ticket = headless.invite_ticket().await.unwrap().to_string();
    let pending = tokio::spawn({
        let a = a.node.clone();
        async move { tokio::time::timeout(Duration::from_secs(3), a.add_contact(&ticket)).await }
    });
    wait_for(|| !headless.pending_requests().is_empty()).await;
    assert!(headless
        .contacts()
        .iter()
        .all(|c| c.display_name != "alice" || !c.is_mutual()));
    drop(pending);
}
