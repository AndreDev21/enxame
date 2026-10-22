//! Fluxo de rede do carry: discar, aceitar, escolher carriers e oferecer mensagens pendentes.

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use iroh::endpoint::{Connection, RecvStream, SendStream};

use super::{Inner, CLOSE_CODE_NOT_CONTACT};
use crate::{
    carry::{CarrySession, CarryStats, OfferSet, CARRIERS_PER_MESSAGE},
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
    offers: Vec<OfferSet>,
) -> Result<CarryStats> {
    let now = unix_now_ms();
    let _ = inner.store.run(move |db| store::expire(db, now)).await;
    let contacts = inner.mutual_contacts();
    let co_members = inner.co_members().await?;
    let groups_before = inner.groups_now().await?;
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
    let on_blob_offer = |offerer: &Contact, blobs: Vec<crate::proto::BlobOffer>| {
        let _ = inner.prefetch_tx.send(super::files::PrefetchJob {
            offerer: offerer.clone(),
            blobs,
        });
    };
    let session = CarrySession {
        store: &inner.store,
        identity: &inner.identity,
        peer: c,
        contacts: &contacts,
        co_members: &co_members,
        groups: groups_before.iter().filter(|g| g.active).cloned().collect(),
        limits: CarryLimits::for_role(inner.role(), &inner.cfg.budget),
        max_frame: inner.cfg.max_frame_bytes,
        limiter: limiter.as_ref(),
        throughput: &inner.throughput,
        tick: &tick,
        on_received: &on_received,
        max_file_bytes: inner.cfg.max_file_mb.saturating_mul(1024 * 1024),
        on_blob_offer: &on_blob_offer,
        offers,
    };
    let stats = session.run(recv, send).await?;
    super::groups::emit_group_changes(inner, &groups_before).await;
    Ok(stats)
}

/// Disca o contato em `enxame/carry/1` e troca entregas, recibos e ofertas.
pub(super) async fn carry_with(
    inner: &Inner,
    c: &Contact,
    offers: Vec<OfferSet>,
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
    let contact = inner.peer_by_endpoint(&remote).await;
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
    // Envelopes com o conteúdo decifrado: os `FileRef` dizem que blobs acompanham a mensagem.
    let with_plain = inner
        .store
        .run(move |db| store::envelopes_with_plaintext(db, &ids2))
        .await?;
    let max_file = inner.cfg.max_file_mb.saturating_mul(1024 * 1024);
    let mut blob_of: HashMap<[u8; 32], crate::proto::BlobOffer> = HashMap::new();
    let mut envs: Vec<Envelope> = Vec::new();
    for (env, plain) in with_plain {
        if env.kind == crate::crypto::EnvelopeKind::FileRef {
            if let Some(fr) = plain.and_then(|p| crate::FileRef::decode(&p, max_file).ok()) {
                blob_of.insert(
                    env.id,
                    crate::proto::BlobOffer {
                        hash: fr.blob_hash,
                        enc_size: fr.enc_size(),
                    },
                );
            }
        }
        envs.push(env);
    }
    let ids3 = ids.clone();
    let (common, mut offered) = inner
        .store
        .run(move |db| {
            let common = store::common_get(db, &rpk)?;
            // Para cada mensagem, quem já aceitou carregá-la.
            let mut offered: HashMap<[u8; 32], HashSet<[u8; 32]>> = HashMap::new();
            for id in &ids3 {
                offered.insert(*id, store::offered_carriers(db, id)?.into_iter().collect());
            }
            Ok((common, offered))
        })
        .await?;
    let mine = inner.peers().await?;
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
        let cpk = cand.identity_pk;
        // Só o que este carrier ainda não guarda e que ainda não tem carriers suficientes.
        let todo: Vec<Envelope> = envs
            .iter()
            .filter(|e| {
                let have = offered.get(&e.id);
                have.is_none_or(|h| h.len() < CARRIERS_PER_MESSAGE && !h.contains(&cpk))
            })
            .cloned()
            .collect();
        if todo.is_empty() {
            continue;
        }
        let blobs: Vec<crate::proto::BlobOffer> = todo
            .iter()
            .filter_map(|e| blob_of.get(&e.id).cloned())
            .collect();
        let offer = OfferSet {
            to: recipient.clone(),
            envelopes: todo,
            blobs,
        };
        match carry_with(inner, &cand, vec![offer]).await {
            Ok(stats) => {
                let _ = inner
                    .store
                    .run(move |db| store::reliability_record(db, &cpk, true, None, unix_now_ms()))
                    .await;
                for (_, ids) in &stats.accepted {
                    for id in ids {
                        offered.entry(*id).or_default().insert(cpk);
                    }
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
    // Carriers distintos que guardam alguma das mensagens pendentes.
    let holding: HashSet<[u8; 32]> = offered.values().flatten().copied().collect();
    Ok(holding.len())
}
