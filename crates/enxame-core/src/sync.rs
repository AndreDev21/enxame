//! Sessão de sincronização por conversa (spec 7.3, ADR 0005).
//!
//! A sessão é simétrica e genérica sobre `AsyncRead`/`AsyncWrite`, então roda igual sobre um
//! stream QUIC ou sobre `tokio::io::duplex` nos testes.
//!
//! Fluxo: cada lado envia `Heads` por conversa; quem vê uma ponta desconhecida pagina
//! `RangeQuery` → `Ids` → `Want` → `Envelopes`. Só se aceita o que foi pedido. Um lado termina
//! com `Bye` quando já recebeu os `Heads` do par e não tem pedidos em aberto, e a sessão acaba
//! quando os dois enviaram e receberam `Bye`.

use std::{collections::HashSet, time::Duration};

use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    crypto::{Envelope, EnvelopeKind, MAX_TEXT_BYTES},
    hlc::Hlc,
    identity::{unix_now_ms, Identity},
    proto::{read_frame, write_frame, SyncMsg},
    store::{self, Store},
    Error, Result, Role,
};

/// HLC de envelopes além disto no futuro é recusado (relógio errado ou ataque ao HLC).
pub const MAX_FUTURE_MS: u64 = 24 * 3600 * 1000;
const PAGE_LIMIT: usize = 1000;
const WANT_CHUNK: usize = 256;
const MAX_HEADS: usize = 256;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Resultado de uma sessão.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncStats {
    /// Envelopes enviados ao par.
    pub sent: usize,
    /// Envelopes novos recebidos e guardados.
    pub received: usize,
    /// Envelopes recebidos e descartados (inválidos, não autorizados ou duplicados).
    pub rejected: usize,
    /// Bytes de envelopes enviados ao par (base da estimativa passiva de upload).
    pub sent_bytes: usize,
}

/// O que acontece com um envelope recebido.
#[derive(Debug, PartialEq, Eq)]
pub enum Ingest {
    Stored,
    Duplicate,
}

/// Valida, decifra e guarda um envelope vindo do par. Qualquer falha descarta o envelope.
///
/// Ordem: estrutura/id/assinatura → conversa permitida → autor é o par ou eu → HLC não está no
/// futuro → tipo suportado → deduplicação → decifra → texto válido → grava.
pub async fn ingest(
    store: &Store,
    identity: &Identity,
    peer_pk: &[u8; 32],
    allowed: &[[u8; 32]],
    env: Envelope,
) -> Result<Ingest> {
    env.verify()?;
    if !allowed.contains(&env.conversation_id) {
        return Err(Error::Protocol(
            "conversa não permitida com este par".into(),
        ));
    }
    if env.author != *peer_pk && env.author != identity.identity_pk() {
        return Err(Error::Protocol("autor não é o par nem eu".into()));
    }
    let now = unix_now_ms();
    if env.hlc.ms > now.saturating_add(MAX_FUTURE_MS) {
        return Err(Error::Protocol("HLC no futuro".into()));
    }
    if env.kind != EnvelopeKind::Text {
        return Err(Error::Protocol(
            "tipo de envelope ainda não suportado".into(),
        ));
    }
    let id = env.id;
    if store.run(move |c| store::envelope_exists(c, &id)).await? {
        return Ok(Ingest::Duplicate);
    }
    let plain = env.open(identity)?;
    if plain.len() > MAX_TEXT_BYTES || std::str::from_utf8(&plain).is_err() {
        return Err(Error::Protocol("texto inválido".into()));
    }
    let outgoing = env.author == identity.identity_pk();
    let inserted = store
        .run(move |c| store::envelope_insert(c, &env, Some(&plain), outgoing, now))
        .await?;
    Ok(if inserted {
        Ingest::Stored
    } else {
        Ingest::Duplicate
    })
}

/// Valida, decifra e guarda um envelope **de terceiro** que um carrier entregou. Diferente de
/// [`ingest`], o autor não é quem entrega: basta ser um contato mútuo meu. Devolve a identidade
/// do autor para o recibo.
pub async fn ingest_carried(
    store: &Store,
    identity: &Identity,
    contacts: &[crate::contacts::Contact],
    env: Envelope,
) -> Result<(Ingest, [u8; 32])> {
    env.verify()?;
    let me = identity.identity_pk();
    let author = contacts
        .iter()
        .find(|c| c.is_mutual() && c.identity_pk == env.author && env.author != me)
        .ok_or_else(|| Error::Protocol("autor do envelope carregado não é meu contato".into()))?;
    if env.conversation_id != crate::crypto::dm_conversation_id(&me, &author.identity_pk) {
        return Err(Error::Protocol(
            "conversa não é a nossa conversa 1:1".into(),
        ));
    }
    let now = unix_now_ms();
    if env.hlc.ms > now.saturating_add(MAX_FUTURE_MS) {
        return Err(Error::Protocol("HLC no futuro".into()));
    }
    if env.kind != EnvelopeKind::Text {
        return Err(Error::Protocol(
            "tipo de envelope ainda não suportado".into(),
        ));
    }
    let (id, pk) = (env.id, author.identity_pk);
    if store.run(move |c| store::envelope_exists(c, &id)).await? {
        return Ok((Ingest::Duplicate, pk));
    }
    let plain = env.open(identity)?;
    if plain.len() > MAX_TEXT_BYTES || std::str::from_utf8(&plain).is_err() {
        return Err(Error::Protocol("texto inválido".into()));
    }
    let inserted = store
        .run(move |c| store::envelope_insert(c, &env, Some(&plain), false, now))
        .await?;
    Ok((
        if inserted {
            Ingest::Stored
        } else {
            Ingest::Duplicate
        },
        pk,
    ))
}

/// Parâmetros de uma sessão.
pub struct Session<'a> {
    pub store: &'a Store,
    pub identity: &'a Identity,
    pub peer_pk: [u8; 32],
    /// Conversas em comum (na Fase 2, só a DM; grupos entram na Fase 5).
    pub conversations: Vec<[u8; 32]>,
    pub max_frame: usize,
    /// Chamado para cada envelope novo guardado.
    pub on_stored: &'a (dyn Fn(&Envelope) + Send + Sync),
    /// Hashes dos meus contatos mútuos (salgados com esta conversa), enviados no início.
    pub common_hashes: Vec<[u8; 32]>,
    /// Recebe os hashes de contatos mútuos do par.
    pub on_common: &'a (dyn Fn(Vec<[u8; 32]>) + Send + Sync),
    /// Recebe o papel anunciado pelo par (`RoleUpdate`).
    pub on_role: &'a (dyn Fn(Role) + Send + Sync),
    /// Se o meu papel mudar durante a sessão, anuncia ao par.
    pub role_updates: Option<tokio::sync::watch::Receiver<Role>>,
}

/// Limite de hashes de contatos em comum por mensagem.
pub const MAX_COMMON_HASHES: usize = 512;

/// Hash de um contato `other` visto da conversa `conv`: só quem conhece a conversa e o contato
/// consegue calculá-lo.
pub fn common_hash(conv: &[u8; 32], other: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"enxame/common/1\0");
    h.update(conv);
    h.update(other);
    *h.finalize().as_bytes()
}

impl Session<'_> {
    async fn send<W: AsyncWrite + Unpin>(&self, w: &mut W, msg: &SyncMsg) -> Result<()> {
        write_frame(w, msg, self.max_frame).await
    }

    /// Roda a sessão até os dois lados enviarem e receberem `Bye`.
    pub async fn run<R, W>(&self, r: R, w: &mut W) -> Result<SyncStats>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin,
    {
        // Um leitor dedicado entrega os frames por canal: assim a leitura é cancelável e o laço
        // principal também reage a mudanças de papel enquanto espera o par.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<SyncMsg>>(8);
        let max = self.max_frame;
        let reader = tokio::spawn(async move {
            let mut r = r;
            loop {
                let msg = read_frame::<_, SyncMsg>(&mut r, max).await;
                let failed = msg.is_err();
                if tx.send(msg).await.is_err() || failed {
                    break;
                }
            }
        });
        let result = self.run_inner(&mut rx, w).await;
        reader.abort();
        result
    }

    async fn run_inner<W>(
        &self,
        rx: &mut tokio::sync::mpsc::Receiver<Result<SyncMsg>>,
        w: &mut W,
    ) -> Result<SyncStats>
    where
        W: AsyncWrite + Unpin,
    {
        let mut role_rx = self.role_updates.clone();
        let mut stats = SyncStats::default();
        if !self.common_hashes.is_empty() {
            let hashes = self
                .common_hashes
                .iter()
                .copied()
                .take(MAX_COMMON_HASHES)
                .collect();
            self.send(w, &SyncMsg::Common { hashes }).await?;
        }
        for conv in &self.conversations {
            let conv = *conv;
            let (heads, (count, hlc_max), digest) = self
                .store
                .run(move |c| {
                    Ok((
                        store::heads(c, &conv)?,
                        store::conversation_summary(c, &conv)?,
                        store::conversation_digest(c, &conv)?,
                    ))
                })
                .await?;
            let heads = heads.into_iter().take(MAX_HEADS).collect();
            self.send(
                w,
                &SyncMsg::Heads {
                    conversation_id: conv,
                    heads,
                    count,
                    hlc_max,
                    digest,
                },
            )
            .await?;
        }

        let mut heads_received: HashSet<[u8; 32]> = HashSet::new();
        let mut outstanding: usize = 0;
        let mut wanted: HashSet<[u8; 32]> = HashSet::new();
        let (mut bye_sent, mut bye_recv) = (false, false);

        loop {
            if !bye_sent && heads_received.len() == self.conversations.len() && outstanding == 0 {
                self.send(w, &SyncMsg::Bye).await?;
                bye_sent = true;
            }
            if bye_sent && bye_recv {
                return Ok(stats);
            }
            let msg: SyncMsg = tokio::select! {
                got = tokio::time::timeout(IDLE_TIMEOUT, rx.recv()) => match got {
                    Err(_) => return Err(Error::Timeout("sessão de sincronização")),
                    Ok(None) => return Err(Error::Net("o par encerrou a sessão".into())),
                    Ok(Some(msg)) => msg?,
                },
                changed = async {
                    match role_rx.as_mut() {
                        Some(rx) => rx.changed().await.is_ok(),
                        None => std::future::pending().await,
                    }
                } => {
                    if changed {
                        if let Some(rx) = role_rx.as_mut() {
                            let role = *rx.borrow_and_update();
                            self.send(w, &SyncMsg::RoleUpdate { role }).await?;
                        }
                    } else {
                        role_rx = None;
                    }
                    continue;
                }
            };
            match msg {
                SyncMsg::Heads {
                    conversation_id: conv,
                    digest: theirs,
                    ..
                } => {
                    if !self.conversations.contains(&conv) || !heads_received.insert(conv) {
                        continue;
                    }
                    // Ter a ponta do par não basta (podem faltar ancestrais): compara o resumo
                    // do conjunto inteiro e puxa a lista de ids se diferir.
                    let ours = self
                        .store
                        .run(move |c| store::conversation_digest(c, &conv))
                        .await?;
                    if ours != theirs {
                        outstanding += 1;
                        self.send(
                            w,
                            &SyncMsg::RangeQuery {
                                conversation_id: conv,
                                since_hlc: Hlc::ZERO,
                                limit: PAGE_LIMIT as u32,
                            },
                        )
                        .await?;
                    }
                }
                SyncMsg::RangeQuery {
                    conversation_id: conv,
                    since_hlc,
                    limit,
                } => {
                    let (ids, next) = if self.conversations.contains(&conv) {
                        let limit = (limit as usize).clamp(1, PAGE_LIMIT);
                        self.store
                            .run(move |c| store::ids_since(c, &conv, since_hlc, limit))
                            .await?
                    } else {
                        (vec![], None)
                    };
                    self.send(
                        w,
                        &SyncMsg::Ids {
                            conversation_id: conv,
                            ids,
                            next,
                        },
                    )
                    .await?;
                }
                SyncMsg::Ids {
                    conversation_id: conv,
                    ids,
                    next,
                } => {
                    if outstanding == 0 || !self.conversations.contains(&conv) {
                        return Err(Error::Protocol("Ids não solicitado".into()));
                    }
                    outstanding -= 1;
                    let ids: Vec<_> = ids.into_iter().take(PAGE_LIMIT).collect();
                    let missing = self
                        .store
                        .run(move |c| {
                            let mut m = Vec::new();
                            for id in ids {
                                if !store::envelope_exists(c, &id)? {
                                    m.push(id);
                                }
                            }
                            Ok(m)
                        })
                        .await?;
                    for chunk in missing.chunks(WANT_CHUNK) {
                        wanted.extend(chunk.iter().copied());
                        outstanding += 1;
                        self.send(
                            w,
                            &SyncMsg::Want {
                                conversation_id: conv,
                                ids: chunk.to_vec(),
                            },
                        )
                        .await?;
                    }
                    if let Some(next) = next {
                        outstanding += 1;
                        self.send(
                            w,
                            &SyncMsg::RangeQuery {
                                conversation_id: conv,
                                since_hlc: next,
                                limit: PAGE_LIMIT as u32,
                            },
                        )
                        .await?;
                    }
                }
                SyncMsg::Want {
                    conversation_id: conv,
                    ids,
                } => {
                    let envs = if self.conversations.contains(&conv) {
                        let ids: Vec<_> = ids.into_iter().take(WANT_CHUNK).collect();
                        self.store
                            .run(move |c| store::envelopes_by_ids(c, &ids))
                            .await?
                    } else {
                        vec![]
                    };
                    stats.sent += envs.len();
                    for batch in self.batches(envs)? {
                        if let SyncMsg::Envelopes { envelopes, .. } = &batch {
                            for e in envelopes {
                                stats.sent_bytes += postcard::to_stdvec(e).map_or(0, |v| v.len());
                            }
                        }
                        self.send(w, &batch).await?;
                    }
                }
                SyncMsg::Envelopes { envelopes, last } => {
                    for env in envelopes {
                        if !wanted.remove(&env.id) {
                            stats.rejected += 1;
                            tracing::debug!("envelope não solicitado descartado");
                            continue;
                        }
                        let stored = env.clone();
                        match ingest(
                            self.store,
                            self.identity,
                            &self.peer_pk,
                            &self.conversations,
                            env,
                        )
                        .await
                        {
                            Ok(Ingest::Stored) => {
                                stats.received += 1;
                                (self.on_stored)(&stored);
                            }
                            Ok(Ingest::Duplicate) => stats.rejected += 1,
                            Err(e) => {
                                stats.rejected += 1;
                                tracing::warn!("envelope descartado: {e}");
                            }
                        }
                    }
                    if last {
                        outstanding = outstanding.saturating_sub(1);
                    }
                }
                SyncMsg::Bye => bye_recv = true,
                SyncMsg::Common { hashes } => {
                    (self.on_common)(hashes.into_iter().take(MAX_COMMON_HASHES).collect());
                }
                SyncMsg::RoleUpdate { role } => (self.on_role)(role),
            }
        }
    }

    /// Divide `envs` em frames que cabem em `max_frame`; o último leva `last = true` (mesmo
    /// vazio, para o par fechar a contagem de pedidos em aberto).
    fn batches(&self, envs: Vec<Envelope>) -> Result<Vec<SyncMsg>> {
        let budget = self.max_frame.saturating_sub(1024);
        let mut out = Vec::new();
        let mut cur: Vec<Envelope> = Vec::new();
        let mut size = 0usize;
        for env in envs {
            let n = postcard::to_stdvec(&env)?.len();
            if n > budget {
                continue; // não cabe nunca num frame; o par não o receberá
            }
            if size + n > budget && !cur.is_empty() {
                out.push(SyncMsg::Envelopes {
                    envelopes: std::mem::take(&mut cur),
                    last: false,
                });
                size = 0;
            }
            size += n;
            cur.push(env);
        }
        out.push(SyncMsg::Envelopes {
            envelopes: cur,
            last: true,
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crypto::{dm_conversation_id, Recipient},
        order::order_ids,
    };
    use proptest::prelude::*;

    struct Node {
        id: Identity,
        store: Store,
    }

    impl Node {
        fn new() -> Self {
            Self {
                id: Identity::generate(),
                store: Store::open_in_memory().unwrap(),
            }
        }
        fn rcpt(&self) -> Recipient {
            Recipient {
                identity_pk: self.id.identity_pk(),
                enc_pk: self.id.enc_pk(),
            }
        }
    }

    fn conv(a: &Node, b: &Node) -> [u8; 32] {
        dm_conversation_id(&a.id.identity_pk(), &b.id.identity_pk())
    }

    fn make(from: &Node, to: &Node, hlc_ms: u64, parents: Vec<[u8; 32]>, text: &str) -> Envelope {
        Envelope::create(
            &from.id,
            conv(from, to),
            Hlc::new(hlc_ms, 0),
            parents,
            &[to.rcpt(), from.rcpt()],
            EnvelopeKind::Text,
            text.as_bytes(),
        )
        .unwrap()
    }

    async fn put(n: &Node, envs: &[Envelope]) {
        let me = n.id.identity_pk();
        for e in envs {
            let plain = e.open(&n.id).unwrap();
            let e = e.clone();
            n.store
                .run(move |c| store::envelope_insert(c, &e, Some(&plain), e.author == me, 0))
                .await
                .unwrap();
        }
    }

    async fn ids_of(n: &Node, conv: [u8; 32]) -> Vec<[u8; 32]> {
        let all = n
            .store
            .run(move |c| store::conversation_envelopes(c, &conv))
            .await
            .unwrap();
        let items: Vec<_> = all.iter().map(|s| s.order_item()).collect();
        order_ids(&items)
    }

    async fn sync_pair(a: &Node, b: &Node) -> (SyncStats, SyncStats) {
        let (ea, eb) = tokio::io::duplex(1 << 20);
        let (ra, mut wa) = tokio::io::split(ea);
        let (rb, mut wb) = tokio::io::split(eb);
        let nop = |_: &Envelope| {};
        let sa = Session {
            store: &a.store,
            identity: &a.id,
            peer_pk: b.id.identity_pk(),
            conversations: vec![conv(a, b)],
            max_frame: 1 << 20,
            on_stored: &nop,
            common_hashes: vec![],
            on_common: &|_| {},
            on_role: &|_| {},
            role_updates: None,
        };
        let sb = Session {
            store: &b.store,
            identity: &b.id,
            peer_pk: a.id.identity_pk(),
            conversations: vec![conv(a, b)],
            max_frame: 1 << 20,
            on_stored: &nop,
            common_hashes: vec![],
            on_common: &|_| {},
            on_role: &|_| {},
            role_updates: None,
        };
        let (x, y) = tokio::join!(sa.run(ra, &mut wa), sb.run(rb, &mut wb));
        (x.unwrap(), y.unwrap())
    }

    #[tokio::test]
    async fn empty_conversations_finish_immediately() {
        let (a, b) = (Node::new(), Node::new());
        let (x, y) = sync_pair(&a, &b).await;
        assert_eq!((x, y), (SyncStats::default(), SyncStats::default()));
    }

    #[tokio::test]
    async fn one_sided_history_is_pulled() {
        let (a, b) = (Node::new(), Node::new());
        let e1 = make(&a, &b, 1, vec![], "um");
        let e2 = make(&b, &a, 2, vec![e1.id], "dois");
        let e3 = make(&a, &b, 3, vec![e2.id], "três");
        put(&a, &[e1.clone(), e2.clone(), e3.clone()]).await;
        let (sa, sb) = sync_pair(&a, &b).await;
        assert_eq!((sa.sent, sa.received), (3, 0));
        assert_eq!((sb.sent, sb.received), (0, 3));
        assert_eq!(ids_of(&b, conv(&a, &b)).await, vec![e1.id, e2.id, e3.id]);
        // Segunda sessão: nada a trocar (idempotente, sem duplicar).
        let (sa, sb) = sync_pair(&a, &b).await;
        assert_eq!((sa.sent, sa.received, sb.sent, sb.received), (0, 0, 0, 0));
    }

    #[tokio::test]
    async fn concurrent_messages_merge_on_both_sides() {
        let (a, b) = (Node::new(), Node::new());
        let base = make(&a, &b, 1, vec![], "base");
        put(&a, std::slice::from_ref(&base)).await;
        put(&b, std::slice::from_ref(&base)).await;
        let ea = make(&a, &b, 10, vec![base.id], "de A, offline");
        let eb = make(&b, &a, 10, vec![base.id], "de B, offline");
        put(&a, std::slice::from_ref(&ea)).await;
        put(&b, std::slice::from_ref(&eb)).await;
        sync_pair(&a, &b).await;
        let (oa, ob) = (
            ids_of(&a, conv(&a, &b)).await,
            ids_of(&b, conv(&a, &b)).await,
        );
        assert_eq!(oa.len(), 3);
        assert_eq!(oa, ob, "mesma ordem nos dois lados");
        // Depois do merge, a próxima mensagem referencia as duas pontas.
        let heads = a
            .store
            .run({
                let c = conv(&a, &b);
                move |conn| store::heads(conn, &c)
            })
            .await
            .unwrap();
        assert_eq!(heads.len(), 2);
    }

    #[tokio::test]
    async fn pagination_transfers_more_than_one_page() {
        let (a, b) = (Node::new(), Node::new());
        let mut envs = Vec::new();
        let mut parent = vec![];
        for i in 0..(PAGE_LIMIT as u64 + 300) {
            let e = make(&a, &b, 1 + i, parent.clone(), &format!("m{i}"));
            parent = vec![e.id];
            envs.push(e);
        }
        put(&a, &envs).await;
        let (_, sb) = sync_pair(&a, &b).await;
        assert_eq!(sb.received, envs.len());
        assert_eq!(ids_of(&b, conv(&a, &b)).await.len(), envs.len());
    }

    /// Par malicioso escrito à mão: anuncia uma ponta, responde `Ids` e entrega envelopes ruins.
    async fn evil_peer(
        mut r: impl AsyncRead + Unpin,
        mut w: impl AsyncWrite + Unpin,
        conv: [u8; 32],
        advertised: Envelope,
        delivered: Vec<Envelope>,
    ) {
        let max = 1 << 20;
        write_frame(
            &mut w,
            &SyncMsg::Heads {
                conversation_id: conv,
                heads: vec![advertised.id],
                count: 1,
                hlc_max: advertised.hlc,
                digest: [9; 32],
            },
            max,
        )
        .await
        .unwrap();
        loop {
            let Ok(msg) = read_frame::<_, SyncMsg>(&mut r, max).await else {
                return;
            };
            match msg {
                SyncMsg::RangeQuery { .. } => {
                    write_frame(
                        &mut w,
                        &SyncMsg::Ids {
                            conversation_id: conv,
                            ids: vec![advertised.id],
                            next: None,
                        },
                        max,
                    )
                    .await
                    .unwrap();
                }
                SyncMsg::Want { .. } => {
                    write_frame(
                        &mut w,
                        &SyncMsg::Envelopes {
                            envelopes: delivered.clone(),
                            last: true,
                        },
                        max,
                    )
                    .await
                    .unwrap();
                    write_frame(&mut w, &SyncMsg::Bye, max).await.unwrap();
                }
                SyncMsg::Bye | SyncMsg::Heads { .. } => {}
                _ => {}
            }
        }
    }

    async fn run_against_evil(
        victim: &Node,
        peer: &Node,
        advertised: Envelope,
        delivered: Vec<Envelope>,
    ) -> SyncStats {
        let (ea, eb) = tokio::io::duplex(1 << 20);
        let (ra, mut wa) = tokio::io::split(ea);
        let (rb, wb) = tokio::io::split(eb);
        let c = conv(victim, peer);
        let evil = tokio::spawn(evil_peer(rb, wb, c, advertised, delivered));
        let nop = |_: &Envelope| {};
        let s = Session {
            store: &victim.store,
            identity: &victim.id,
            peer_pk: peer.id.identity_pk(),
            conversations: vec![c],
            max_frame: 1 << 20,
            on_stored: &nop,
            common_hashes: vec![],
            on_common: &|_| {},
            on_role: &|_| {},
            role_updates: None,
        };
        let out = tokio::time::timeout(Duration::from_secs(5), s.run(ra, &mut wa)).await;
        evil.abort();
        out.unwrap_or(Ok(SyncStats::default())).unwrap_or_default()
    }

    async fn count(n: &Node, c: [u8; 32]) -> usize {
        ids_of(n, c).await.len()
    }

    #[tokio::test]
    async fn tampered_envelope_is_rejected_and_not_stored() {
        let (victim, peer) = (Node::new(), Node::new());
        let good = make(&peer, &victim, 5, vec![], "legítima");
        let mut bad = good.clone();
        bad.ciphertext[30] ^= 1; // id e assinatura deixam de conferir
        let stats = run_against_evil(&victim, &peer, good.clone(), vec![bad]).await;
        assert_eq!(stats.received, 0);
        assert_eq!(count(&victim, conv(&victim, &peer)).await, 0);
    }

    #[tokio::test]
    async fn unsolicited_envelope_is_not_stored() {
        let (victim, peer) = (Node::new(), Node::new());
        let advertised = make(&peer, &victim, 5, vec![], "anunciada");
        let extra = make(&peer, &victim, 6, vec![], "enfiada sem pedir");
        let stats =
            run_against_evil(&victim, &peer, advertised.clone(), vec![extra, advertised]).await;
        assert_eq!(stats.received, 1, "só a pedida");
        assert_eq!(count(&victim, conv(&victim, &peer)).await, 1);
    }

    #[tokio::test]
    async fn envelope_from_a_stranger_author_is_rejected() {
        let (victim, peer, stranger) = (Node::new(), Node::new(), Node::new());
        // Assinatura válida, mas o autor não é o par nem eu.
        // Conversa certa, estrutura válida, mas escrita por um terceiro.
        let foreign = Envelope::create(
            &stranger.id,
            conv(&victim, &peer),
            Hlc::new(5, 0),
            vec![],
            &[victim.rcpt()],
            EnvelopeKind::Text,
            b"intruso",
        )
        .unwrap();
        let stats = run_against_evil(&victim, &peer, foreign.clone(), vec![foreign]).await;
        assert_eq!(stats.received, 0);
        assert_eq!(count(&victim, conv(&victim, &peer)).await, 0);
    }

    #[tokio::test]
    async fn envelope_from_the_far_future_is_rejected() {
        let (victim, peer) = (Node::new(), Node::new());
        let far = unix_now_ms() + 3 * MAX_FUTURE_MS;
        let env = make(&peer, &victim, far, vec![], "do futuro");
        let stats = run_against_evil(&victim, &peer, env.clone(), vec![env]).await;
        assert_eq!(stats.received, 0);
    }

    fn plain_session<'a>(
        n: &'a Node,
        peer: &Node,
        on_common: &'a (dyn Fn(Vec<[u8; 32]>) + Send + Sync),
        on_role: &'a (dyn Fn(Role) + Send + Sync),
        roles: Option<tokio::sync::watch::Receiver<Role>>,
        common: Vec<[u8; 32]>,
    ) -> Session<'a> {
        static NOP: fn(&Envelope) = |_| {};
        Session {
            store: &n.store,
            identity: &n.id,
            peer_pk: peer.id.identity_pk(),
            conversations: vec![conv(n, peer)],
            max_frame: 1 << 20,
            on_stored: &NOP,
            common_hashes: common,
            on_common,
            on_role,
            role_updates: roles,
        }
    }

    #[tokio::test]
    async fn role_change_is_announced_to_the_peer_during_a_session() {
        let (a, b) = (Node::new(), Node::new());
        let (ea, eb) = tokio::io::duplex(1 << 20);
        let (ra, mut wa) = tokio::io::split(ea);
        let (mut rb, _wb) = tokio::io::split(eb);
        let (tx, rx) = tokio::sync::watch::channel(Role::Peer);
        let sa = plain_session(&a, &b, &|_| {}, &|_| {}, Some(rx), vec![]);
        let driver = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            tx.send(Role::Relay).unwrap();
            loop {
                match read_frame::<_, SyncMsg>(&mut rb, 1 << 20).await {
                    Ok(SyncMsg::RoleUpdate { role }) => return role,
                    Ok(_) => continue,
                    Err(e) => panic!("sem RoleUpdate: {e}"),
                }
            }
        };
        let announced = tokio::select! {
            _ = sa.run(ra, &mut wa) => panic!("a sessão terminou antes do anúncio"),
            role = driver => role,
        };
        assert_eq!(announced, Role::Relay);
    }

    #[tokio::test]
    async fn peer_role_and_common_contacts_are_delivered_to_callbacks() {
        let (a, b) = (Node::new(), Node::new());
        let c = conv(&a, &b);
        let digest = a
            .store
            .run(move |db| store::conversation_digest(db, &c))
            .await
            .unwrap();
        let (ea, eb) = tokio::io::duplex(1 << 20);
        let (ra, mut wa) = tokio::io::split(ea);
        let (_rb, mut wb) = tokio::io::split(eb);
        let got_common = std::sync::Mutex::new(Vec::new());
        let got_role = std::sync::Mutex::new(Vec::new());
        let on_common = |h: Vec<[u8; 32]>| got_common.lock().unwrap().extend(h);
        let on_role = |r: Role| got_role.lock().unwrap().push(r);
        let sa = plain_session(
            &a,
            &b,
            &on_common,
            &on_role,
            None,
            vec![common_hash(&c, &[7; 32])],
        );
        let max = 1 << 20;
        write_frame(
            &mut wb,
            &SyncMsg::Common {
                hashes: vec![[1; 32], [2; 32]],
            },
            max,
        )
        .await
        .unwrap();
        write_frame(
            &mut wb,
            &SyncMsg::RoleUpdate {
                role: Role::SuperPeer,
            },
            max,
        )
        .await
        .unwrap();
        write_frame(
            &mut wb,
            &SyncMsg::Heads {
                conversation_id: c,
                heads: vec![],
                count: 0,
                hlc_max: Hlc::ZERO,
                digest,
            },
            max,
        )
        .await
        .unwrap();
        write_frame(&mut wb, &SyncMsg::Bye, max).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), sa.run(ra, &mut wa))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*got_common.lock().unwrap(), vec![[1; 32], [2; 32]]);
        assert_eq!(*got_role.lock().unwrap(), vec![Role::SuperPeer]);
    }

    #[test]
    fn common_hash_is_bound_to_the_conversation_and_the_contact() {
        let (c1, c2) = ([1u8; 32], [2u8; 32]);
        let (x, y) = ([7u8; 32], [8u8; 32]);
        assert_eq!(common_hash(&c1, &x), common_hash(&c1, &x));
        assert_ne!(
            common_hash(&c1, &x),
            common_hash(&c2, &x),
            "outra conversa, outro hash"
        );
        assert_ne!(common_hash(&c1, &x), common_hash(&c1, &y));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        #[test]
        fn any_split_of_a_thread_converges(
            n in 1usize..25,
            authors in proptest::collection::vec(any::<bool>(), 25),
            parent_picks in proptest::collection::vec((any::<prop::sample::Index>(), any::<bool>()), 25),
            a_has in proptest::collection::vec(any::<bool>(), 25),
            b_has in proptest::collection::vec(any::<bool>(), 25),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                let (a, b) = (Node::new(), Node::new());
                let mut all: Vec<Envelope> = Vec::new();
                for i in 0..n {
                    let (from, to) = if authors[i] { (&a, &b) } else { (&b, &a) };
                    let mut parents = vec![];
                    if i > 0 {
                        let (pick, two) = &parent_picks[i];
                        parents.push(all[pick.index(i)].id);
                        if *two { parents.push(all[i - 1].id); }
                        parents.sort(); parents.dedup();
                    }
                    all.push(make(from, to, 1 + (i as u64 % 7), parents, &format!("m{i}")));
                }
                let mine: Vec<_> = all.iter().enumerate().filter(|(i, _)| a_has[*i]).map(|(_, e)| e.clone()).collect();
                let theirs: Vec<_> = all.iter().enumerate().filter(|(i, _)| b_has[*i]).map(|(_, e)| e.clone()).collect();
                put(&a, &mine).await;
                put(&b, &theirs).await;
                let union: std::collections::BTreeSet<_> = mine.iter().chain(theirs.iter()).map(|e| e.id).collect();
                sync_pair(&a, &b).await;
                let c = conv(&a, &b);
                let (oa, ob) = (ids_of(&a, c).await, ids_of(&b, c).await);
                prop_assert_eq!(oa.iter().copied().collect::<std::collections::BTreeSet<_>>(), union);
                prop_assert_eq!(&oa, &ob);
                Ok(())
            })?;
        }
    }
}
