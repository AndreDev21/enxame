//! Fluxo de rede do carry: discar, aceitar, escolher carriers e oferecer mensagens pendentes.

use std::{collections::HashSet, time::Duration};

use iroh::endpoint::{Connection, RecvStream, SendStream};

use super::{Inner, CLOSE_CODE_NOT_CONTACT};
use crate::{
    carry::{CarrySession, CarryStats, CARRIERS_PER_MESSAGE},
    contacts::Contact,
    crypto::Envelope,
    identity::unix_now_ms,
    proto::ALPN_CARRY,
    roles::CarryLimits,
    store, Error, Event, Result, Role,
};

const MAX_CANDIDATES: usize = 5;
const OFFER_BATCH: usize = 64;

async fn dial_carry(inner: &Inner, c: &Contact) -> Result<Connection> {
    let dial = inner.endpoint.connect(c.node_addr.clone(), ALPN_CARRY);
    tokio::time::timeout(Duration::from_secs(inner.cfg.dial_timeout_secs), dial)
        .await
        .map_err(|_| Error::Timeout("conectar ao contato (carry)"))?
        .map_err(|e| Error::Net(format!("conectar: {e}")))
}

/// Roda a sessão de carry sobre um par de streams.
async fn run_carry(
    inner: &Inner,
    c: &Contact,
    recv: &mut RecvStream,
    send: &mut SendStream,
    offers: Vec<(Contact, Vec<Envelope>)>,
) -> Result<CarryStats> {
    let now = unix_now_ms();
    let _ = inner.store.run(move |db| store::expire(db, now)).await;
    let contacts = inner.mutual_contacts();
    let limiter = inner.limiter();
    let tick = || inner.clock().tick(unix_now_ms());
    let on_received = |author: &Contact, env: &Envelope| {
        inner.emit(Event::MessageReceived {
            conversation_id: env.conversation_id,
            id: env.id,
            from_name: author.display_name.clone(),
            from_fingerprint: author.fingerprint(),
        });
        inner.clock().observe(env.hlc, unix_now_ms());
    };
    let session = CarrySession {
        store: &inner.store,
        identity: &inner.identity,
        peer: c,
        contacts: &contacts,
        limits: CarryLimits::for_role(inner.role(), &inner.cfg.budget),
        max_frame: inner.cfg.max_frame_bytes,
        limiter: limiter.as_ref(),
        throughput: &inner.throughput,
        tick: &tick,
        on_received: &on_received,
        offers,
    };
    session.run(recv, send).await
}

/// Disca o contato em `enxame/carry/1` e troca entregas, recibos e ofertas.
pub(super) async fn carry_with(
    inner: &Inner,
    c: &Contact,
    offers: Vec<(Contact, Vec<Envelope>)>,
) -> Result<CarryStats> {
    let conn = dial_carry(inner, c).await?;
    let result = async {
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| Error::Net(format!("abrir stream: {e}")))?;
        let stats = run_carry(inner, c, &mut recv, &mut send, offers).await?;
        let _ = send.finish();
        Ok(stats)
    }
    .await;
    conn.close(0u32.into(), b"carry done");
    result
}

/// Lado que aceita a conexão `enxame/carry/1`. Só contatos mútuos.
pub(super) async fn handle_carry(inner: &Inner, conn: &Connection) -> Result<()> {
    let remote = conn.remote_id();
    let contact = inner.book().mutual_by_endpoint(&remote).cloned();
    let Some(contact) = contact else {
        conn.close(CLOSE_CODE_NOT_CONTACT.into(), b"not a mutual contact");
        return Ok(());
    };
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|e| Error::Net(format!("aceitar stream: {e}")))?;
    let stats = run_carry(inner, &contact, &mut recv, &mut send, vec![]).await?;
    let _ = send.finish();
    tracing::debug!(?stats, peer = %contact.fingerprint(), "sessão de carry recebida");
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
    Ok(())
}

/// Papéis em ordem de preferência para escolher carriers (spec 9.2).
fn rank(role: Option<Role>) -> u8 {
    match role {
        Some(Role::SuperPeer) => 4,
        Some(Role::Relay) => 3,
        Some(Role::Peer) => 2,
        None => 1, // nunca visto: tenta depois dos conhecidos
        Some(Role::LeafPlus) => 1,
        Some(Role::Leaf) => 0,
    }
}

/// Oferece as mensagens pendentes para `recipient` a contatos em comum que estejam online,
/// priorizando `SuperPeer > Relay > Peer > LeafPlus` e, dentro do papel, quem mais respondeu.
/// Devolve quantos carriers distintos passaram a guardar cópia (até `K = 3`).
pub(super) async fn offer_to_carriers(inner: &Inner, recipient: &Contact) -> Result<usize> {
    let rpk = recipient.identity_pk;
    let ids = inner
        .store
        .run(move |db| store::pending_list(db, &rpk))
        .await?;
    if ids.is_empty() {
        return Ok(0);
    }
    let ids: Vec<_> = ids.into_iter().take(OFFER_BATCH).collect();
    let ids2 = ids.clone();
    let envs = inner
        .store
        .run(move |db| store::envelopes_by_ids(db, &ids2))
        .await?;
    let ids3 = ids.clone();
    let (common, mut holding) = inner
        .store
        .run(move |db| {
            let common = store::common_get(db, &rpk)?;
            let mut holding: HashSet<[u8; 32]> = HashSet::new();
            for id in &ids3 {
                holding.extend(store::offered_carriers(db, id)?);
            }
            Ok((common, holding))
        })
        .await?;
    let mine = inner.mutual_contacts();
    let mut candidates: Vec<(Contact, u8, f64)> = Vec::new();
    for c in mine
        .into_iter()
        .filter(|c| c.identity_pk != rpk && common.contains(&c.identity_pk))
    {
        let pk = c.identity_pk;
        let (attempts, successes, role) = inner
            .store
            .run(move |db| store::reliability_get(db, &pk))
            .await?;
        if role == Some(Role::Leaf) {
            continue;
        }
        let ratio = if attempts == 0 {
            0.5
        } else {
            successes as f64 / attempts as f64
        };
        candidates.push((c, rank(role), ratio));
    }
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)));

    for (cand, _, _) in candidates.into_iter().take(MAX_CANDIDATES) {
        if holding.len() >= CARRIERS_PER_MESSAGE {
            break;
        }
        let cpk = cand.identity_pk;
        let already = holding.contains(&cpk);
        let todo: Vec<Envelope> = if already { vec![] } else { envs.clone() };
        if todo.is_empty() {
            continue;
        }
        match carry_with(inner, &cand, vec![(recipient.clone(), todo)]).await {
            Ok(stats) => {
                let _ = inner
                    .store
                    .run(move |db| store::reliability_record(db, &cpk, true, None, unix_now_ms()))
                    .await;
                if stats.accepted.iter().any(|(_, ids)| !ids.is_empty()) {
                    holding.insert(cpk);
                }
            }
            Err(e) => {
                tracing::debug!(carrier = %cand.fingerprint(), "carrier inacessível: {e}");
                let _ = inner
                    .store
                    .run(move |db| store::reliability_record(db, &cpk, false, None, unix_now_ms()))
                    .await;
            }
        }
    }
    Ok(holding.len())
}
