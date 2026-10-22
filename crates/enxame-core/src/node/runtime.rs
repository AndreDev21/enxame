//! Tarefas de fundo dos papéis: leitura de sinais, histerese, NAT e contabilidade.

use std::{sync::atomic::Ordering, time::Duration};

use super::{ContributionSummary, Inner};
use crate::{
    carry::current_month,
    config::RoleMode,
    net::{self, NatKind},
    roles::{target_role, RolePolicy, Signals},
    store, Event, Result, Role,
};

const NAT_REFRESH: Duration = Duration::from_secs(300);
const NAT_TIMEOUT: Duration = Duration::from_secs(10);

/// Lê os sinais, decide o papel-alvo, aplica a histerese e anuncia a mudança. `at` é o tempo
/// monotônico desde o início (injetável em testes).
pub(super) async fn tick_roles(inner: &Inner, at: Duration) -> Option<(Role, Role)> {
    let before = inner.role();
    let after = match inner.role_mode {
        RoleMode::Fixed(r) => r,
        RoleMode::Auto => {
            let signals = inner.signals.clone();
            let platform = tokio::task::spawn_blocking(move || signals.read())
                .await
                .ok()?;
            let month = current_month();
            let used = inner
                .store
                .run(move |c| store::contribution_month_bytes(c, month))
                .await
                .unwrap_or(0);
            let cap = inner
                .cfg
                .budget
                .max_contrib_gb_month
                .saturating_mul(1_000_000_000);
            let signals = Signals {
                nat_kind: *inner.nat.lock().unwrap_or_else(|p| p.into_inner()),
                upload_kbps_est: inner.throughput.kbps(),
                platform,
                session_uptime: at,
                foreground: true,
                game_mode: inner.game_mode.load(Ordering::Relaxed),
            };
            let policy = RolePolicy {
                contribution_enabled: inner.contribute.load(Ordering::Relaxed),
                budget_exceeded: used >= cap,
            };
            let target = target_role(&signals, &policy);
            inner
                .engine
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .update(target, at);
            inner
                .engine
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .current()
        }
    };
    if after == before {
        return None;
    }
    // Anuncia aos contatos com sessão aberta (`RoleUpdate`) e a quem usa o stream de eventos.
    inner.role_tx.send_replace(after);
    inner.emit(Event::RoleChanged {
        from: before,
        to: after,
    });
    tracing::info!(?before, ?after, "papel mudou");
    Some((before, after))
}

/// Reavalia o papel periodicamente.
pub(super) async fn role_loop(inner: std::sync::Arc<Inner>) {
    let mut tick = tokio::time::interval(Duration::from_secs(inner.cfg.role_interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        tick_roles(&inner, inner.started.elapsed()).await;
    }
}

/// Atualiza o tipo de NAT (sondagem QAD do iroh) de tempos em tempos.
pub(super) async fn nat_loop(inner: std::sync::Arc<Inner>) {
    loop {
        let report = net::nat_report(&inner.endpoint, NAT_TIMEOUT).await;
        let kind: NatKind = report.kind;
        *inner.nat.lock().unwrap_or_else(|p| p.into_inner()) = kind;
        tokio::time::sleep(NAT_REFRESH).await;
    }
}

pub(super) async fn contribution_summary(inner: &Inner) -> Result<ContributionSummary> {
    let month = current_month();
    let (rows, (items, bytes)) = inner
        .store
        .run(move |c| {
            let rows = store::contribution_by_contact(c, month)?;
            let items = store::carry_count(c)?;
            let (total, _) = store::carry_usage(c, &[0; 32])?;
            Ok((rows, (items, total)))
        })
        .await?;
    let names = inner.book().list().to_vec();
    let by_contact: Vec<_> = rows
        .iter()
        .map(|(pk, b, i)| {
            let name = names.iter().find(|c| &c.identity_pk == pk).map_or_else(
                || crate::identity::fingerprint(pk),
                |c| c.display_name.clone(),
            );
            (name, *b, *i)
        })
        .collect();
    Ok(ContributionSummary {
        month,
        bytes_served: rows.iter().map(|r| r.1).sum(),
        items_delivered: rows.iter().map(|r| r.2).sum(),
        budget_bytes: inner
            .cfg
            .budget
            .max_contrib_gb_month
            .saturating_mul(1_000_000_000),
        by_contact,
        carry_items: items,
        carry_bytes: bytes,
    })
}
