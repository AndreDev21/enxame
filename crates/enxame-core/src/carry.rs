//! Store-and-forward (spec 9, ADR 0006): guardar, entregar e expurgar envelopes de terceiros.
//!
//! A sessão (`enxame/carry/1`) é simétrica e genérica sobre `AsyncRead`/`AsyncWrite`. Cada lado:
//! 1. manda os recibos que conhece (`Receipts`) para o par expurgar cópias;
//! 2. manda os envelopes que carrega **para o par** (`Deliver`);
//! 3. manda as ofertas que quer fazer (`Offer`: "carregue isto para fulano").
//!
//! Cada `Deliver` recebe um `Receipts` de volta e cada `Offer` um `Accept`/`Reject`. Um lado
//! envia `Bye` quando não espera mais respostas; a sessão acaba quando os dois enviaram e
//! receberam `Bye`.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    contacts::Contact,
    crypto::{dm_conversation_id, Envelope, EnvelopeKind, Recipient, MAX_PARENTS},
    hlc::Hlc,
    identity::{unix_now_ms, Identity},
    proto::{read_frame, write_frame, BlobOffer, CarryMsg, RejectReason},
    roles::{CarryLimits, RateLimiter, ThroughputEstimator},
    store::{self, Store},
    sync::{ingest_carried, Ingest, MAX_FUTURE_MS},
    Error, Result,
};

/// Quantos recibos recentes cada lado espalha por sessão.
const RECEIPTS_PER_SESSION: usize = 256;
/// Recibos valem por tanto tempo quanto o maior TTL de carry.
pub const RECEIPT_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
/// Máximo de blobs numa oferta.
const MAX_BLOB_OFFERS: usize = 64;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Carriers por mensagem (spec 9.2).
pub const CARRIERS_PER_MESSAGE: usize = 3;

/// Resultado de uma sessão de carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CarryStats {
    /// Envelopes que entreguei ao par (eu era o carrier).
    pub delivered: usize,
    /// Envelopes de terceiros que recebi do par e guardei (eu era o destinatário).
    pub received: usize,
    /// Cópias que expurguei por causa de recibos.
    pub purged: usize,
    /// Recibos novos que aprendi.
    pub receipts_learned: usize,
    /// Ofertas aceitas pelo par: `(destinatário, ids)`.
    pub accepted: Vec<([u8; 32], Vec<[u8; 32]>)>,
    /// Ofertas que o par recusou, com o motivo.
    pub rejected: Vec<RejectReason>,
    /// Ofertas que eu aceitei do par (ids guardados).
    pub stored_for_others: usize,
}

/// Cria o recibo de `ids` (autoria do contato `author`): envelope `kind = Receipt` assinado por
/// mim, com os ids confirmados em `parents` (em claro: o carrier precisa lê-los) e conteúdo vazio.
pub fn make_receipt(
    identity: &Identity,
    author: &Contact,
    ids: &[[u8; 32]],
    hlc: Hlc,
) -> Result<Envelope> {
    Envelope::create(
        identity,
        dm_conversation_id(&identity.identity_pk(), &author.identity_pk),
        hlc,
        ids.to_vec(),
        &[Recipient {
            identity_pk: author.identity_pk,
            enc_pk: author.enc_pk,
        }],
        EnvelopeKind::Receipt,
        b"",
    )
}

/// Um recibo é utilizável se é estruturalmente válido, do tipo certo e de um contato mútuo.
fn receipt_author_ok(
    env: &Envelope,
    contacts: &[Contact],
    co_members: &[Contact],
    me: &[u8; 32],
) -> bool {
    env.verify().is_ok()
        && env.kind == EnvelopeKind::Receipt
        && !env.parents.is_empty()
        && env.author != *me
        && contacts
            .iter()
            .chain(co_members)
            .any(|c| c.is_mutual() && c.identity_pk == env.author)
}

/// O que quero que o par carregue para um destinatário.
#[derive(Debug, Clone)]
pub struct OfferSet {
    pub to: Contact,
    pub envelopes: Vec<Envelope>,
    /// Arquivos referenciados pelos `FileRef` dentre os envelopes.
    pub blobs: Vec<BlobOffer>,
}

/// Parâmetros de uma sessão de carry.
pub struct CarrySession<'a> {
    pub store: &'a Store,
    pub identity: &'a Identity,
    pub peer: &'a Contact,
    /// Meus contatos mútuos (autores 1:1 e destinatários válidos).
    pub contacts: &'a [Contact],
    /// Co-membros de grupo que não são contatos: também podem ser autores/destinatários, mas só
    /// de mensagens de grupo (ADR 0009).
    pub co_members: &'a [Contact],
    /// Grupos ativos, para autorizar autores de mensagens de grupo.
    pub groups: Vec<crate::groups::StoredGroup>,
    pub limits: CarryLimits,
    pub max_frame: usize,
    pub limiter: &'a RateLimiter,
    pub throughput: &'a ThroughputEstimator,
    /// HLC para os recibos que eu criar.
    pub tick: &'a (dyn Fn() -> Hlc + Send + Sync),
    /// Chamado para cada mensagem nova entregue a mim (para eventos/relógio).
    pub on_received: &'a (dyn Fn(&Contact, &Envelope) + Send + Sync),
    /// Maior arquivo que aceitamos receber (`FileRef`).
    pub max_file_bytes: u64,
    /// Chamado quando aceito uma oferta com blobs: `(quem ofereceu, blobs)`.
    pub on_blob_offer: &'a (dyn Fn(&Contact, Vec<BlobOffer>) + Send + Sync),
    /// O que quero que o par carregue.
    pub offers: Vec<OfferSet>,
}

fn month_of(ms: u64) -> u32 {
    // AAAAMM a partir de ms desde a época (calendário civil, UTC).
    let days = (ms / 86_400_000) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y * 100 + m) as u32
}

/// Mês corrente (`AAAAMM`, UTC).
pub fn current_month() -> u32 {
    month_of(unix_now_ms())
}

impl CarrySession<'_> {
    async fn send<W: AsyncWrite + Unpin>(&self, w: &mut W, msg: &CarryMsg) -> Result<()> {
        write_frame(w, msg, self.max_frame).await
    }

    /// Roda a sessão até os dois lados enviarem e receberem `Bye`.
    pub async fn run<R, W>(&self, r: &mut R, w: &mut W) -> Result<CarryStats>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let peer_pk = self.peer.identity_pk;
        let mut stats = CarryStats::default();
        let budget = self.max_frame.saturating_sub(2048);

        // 1. Recibos que conheço.
        let receipts = self
            .store
            .run(|c| store::receipts_recent(c, RECEIPTS_PER_SESSION))
            .await?;
        let receipts = fit(receipts, budget)?;
        if !receipts.is_empty() {
            self.send(
                w,
                &CarryMsg::Receipts {
                    receipts,
                    reply: false,
                },
            )
            .await?;
        }

        // 2. O que carrego para o par.
        let mut awaiting_deliver = false;
        let queued = self
            .store
            .run(move |c| store::carry_for(c, &peer_pk))
            .await?;
        let deliver = fit(queued, budget)?;
        if !deliver.is_empty() {
            let bytes: usize = deliver
                .iter()
                .map(|e| postcard::to_stdvec(e).map_or(0, |v| v.len()))
                .sum();
            let started = Instant::now();
            let n = deliver.len();
            self.send(w, &CarryMsg::Deliver { envelopes: deliver })
                .await?;
            self.limiter.acquire(bytes).await;
            self.throughput.record(bytes, started.elapsed());
            let month = current_month();
            self.store
                .run(move |c| store::contribution_add(c, month, &peer_pk, bytes as u64, n as u64))
                .await?;
            stats.delivered = n;
            awaiting_deliver = true;
        }

        // 3. Ofertas.
        let mut offered: Vec<([u8; 32], Vec<[u8; 32]>)> = Vec::new();
        for o in &self.offers {
            let envs = fit(o.envelopes.clone(), budget)?;
            if envs.is_empty() {
                continue;
            }
            offered.push((o.to.identity_pk, envs.iter().map(|e| e.id).collect()));
            self.send(
                w,
                &CarryMsg::Offer {
                    for_recipient: o.to.identity_pk,
                    envelopes: envs,
                    blobs: o.blobs.iter().take(MAX_BLOB_OFFERS).cloned().collect(),
                },
            )
            .await?;
        }
        let mut offer_queue: std::collections::VecDeque<_> = offered.into();

        let (mut bye_sent, mut bye_recv) = (false, false);
        loop {
            if !bye_sent && !awaiting_deliver && offer_queue.is_empty() {
                self.send(w, &CarryMsg::Bye).await?;
                bye_sent = true;
            }
            if bye_sent && bye_recv {
                return Ok(stats);
            }
            let msg: CarryMsg = tokio::time::timeout(IDLE_TIMEOUT, read_frame(r, self.max_frame))
                .await
                .map_err(|_| Error::Timeout("sessão de carry"))??;
            match msg {
                CarryMsg::Receipts { receipts, reply } => {
                    if reply {
                        if !awaiting_deliver {
                            return Err(Error::Protocol(
                                "Receipts de resposta não solicitado".into(),
                            ));
                        }
                        awaiting_deliver = false;
                    }
                    let (learned, purged) = self.learn_receipts(receipts).await?;
                    stats.receipts_learned += learned;
                    stats.purged += purged;
                }
                CarryMsg::Deliver { envelopes } => {
                    let (got, reply) = self.take_delivery(envelopes).await?;
                    stats.received += got;
                    self.send(
                        w,
                        &CarryMsg::Receipts {
                            receipts: reply,
                            reply: true,
                        },
                    )
                    .await?;
                }
                CarryMsg::Offer {
                    for_recipient,
                    envelopes,
                    blobs,
                } => {
                    let reply = self
                        .consider_offer(for_recipient, envelopes, &mut stats)
                        .await?;
                    // Aceitei guardar alguma coisa: se o meu papel replica arquivos, busco os blobs.
                    if matches!(&reply, CarryMsg::Accept { ids } if !ids.is_empty())
                        && self.limits.accepts_files
                        && !blobs.is_empty()
                    {
                        let blobs = blobs.into_iter().take(MAX_BLOB_OFFERS).collect();
                        (self.on_blob_offer)(self.peer, blobs);
                    }
                    self.send(w, &reply).await?;
                }
                CarryMsg::Accept { ids } => {
                    let Some((to, offered_ids)) = offer_queue.pop_front() else {
                        return Err(Error::Protocol("Accept não solicitado".into()));
                    };
                    let ok: Vec<_> = ids
                        .into_iter()
                        .filter(|i| offered_ids.contains(i))
                        .collect();
                    let now = unix_now_ms();
                    let ok2 = ok.clone();
                    self.store
                        .run(move |c| {
                            for id in &ok2 {
                                store::offer_record(c, id, &peer_pk, now)?;
                            }
                            Ok(())
                        })
                        .await?;
                    stats.accepted.push((to, ok));
                }
                CarryMsg::Reject { reason } => {
                    if offer_queue.pop_front().is_none() {
                        return Err(Error::Protocol("Reject não solicitado".into()));
                    }
                    stats.rejected.push(reason);
                }
                CarryMsg::Bye => bye_recv = true,
            }
        }
    }

    /// Contato ou co-membro com esta identidade.
    fn peer_named(&self, pk: &[u8; 32]) -> Option<&Contact> {
        self.contacts
            .iter()
            .chain(self.co_members)
            .find(|c| &c.identity_pk == pk)
    }

    /// Valida e aplica recibos do par. Devolve `(novos, cópias expurgadas)`.
    async fn learn_receipts(&self, receipts: Vec<Envelope>) -> Result<(usize, usize)> {
        let me = self.identity.identity_pk();
        let (mut learned, mut purged) = (0, 0);
        for r in receipts.into_iter().take(RECEIPTS_PER_SESSION) {
            if !receipt_author_ok(&r, self.contacts, self.co_members, &me)
                || r.parents.len() > MAX_PARENTS
            {
                continue;
            }
            let now = unix_now_ms();
            if r.hlc.ms > now.saturating_add(MAX_FUTURE_MS) {
                continue;
            }
            let ttl = RECEIPT_TTL.as_millis() as u64;
            let (new, n) = self
                .store
                .run(move |c| store::receipt_store(c, &r, now, ttl))
                .await?;
            learned += usize::from(new);
            purged += n;
        }
        Ok((learned, purged))
    }

    /// Eu sou o destinatário: guarda o que veio e devolve os recibos.
    async fn take_delivery(&self, envelopes: Vec<Envelope>) -> Result<(usize, Vec<Envelope>)> {
        let mut stored = 0;
        let mut by_author: BTreeMap<[u8; 32], Vec<[u8; 32]>> = BTreeMap::new();
        for env in envelopes {
            let copy = env.clone();
            match ingest_carried(
                self.store,
                self.identity,
                self.contacts,
                &self.groups,
                self.max_file_bytes,
                env,
            )
            .await
            {
                Ok((outcome, author)) => {
                    by_author.entry(author).or_default().push(copy.id);
                    if outcome == Ingest::Stored {
                        stored += 1;
                        if let Some(c) = self.peer_named(&author) {
                            (self.on_received)(c, &copy);
                        }
                    }
                }
                Err(e) => tracing::warn!("envelope carregado descartado: {e}"),
            }
        }
        let mut receipts = Vec::new();
        for (author, ids) in by_author {
            let Some(contact) = self.peer_named(&author) else {
                continue;
            };
            for chunk in ids.chunks(MAX_PARENTS) {
                let receipt = make_receipt(self.identity, contact, chunk, (self.tick)())?;
                let (r2, now) = (receipt.clone(), unix_now_ms());
                let ttl = RECEIPT_TTL.as_millis() as u64;
                self.store
                    .run(move |c| store::receipt_store(c, &r2, now, ttl))
                    .await?;
                receipts.push(receipt);
            }
        }
        Ok((stored, receipts))
    }

    /// Decide uma oferta do par: guarda o que couber nos limites do meu papel.
    async fn consider_offer(
        &self,
        for_recipient: [u8; 32],
        envelopes: Vec<Envelope>,
        stats: &mut CarryStats,
    ) -> Result<CarryMsg> {
        let me = self.identity.identity_pk();
        let peer_pk = self.peer.identity_pk;
        if !self.limits.accepts {
            return Ok(CarryMsg::Reject {
                reason: RejectReason::NotCarrying,
            });
        }
        let recipient_ok = for_recipient != peer_pk
            && for_recipient != me
            && self
                .contacts
                .iter()
                .chain(self.co_members)
                .any(|c| c.is_mutual() && c.identity_pk == for_recipient);
        if !recipient_ok {
            return Ok(CarryMsg::Reject {
                reason: RejectReason::NotContact,
            });
        }
        let now = unix_now_ms();
        let (mut total, mut from_peer) = self
            .store
            .run(move |c| store::carry_usage(c, &peer_pk))
            .await?;
        let ttl_ms = self.limits.ttl.as_millis() as u64;
        let mut accepted = Vec::new();
        let (mut eligible, mut over_quota, mut newly) = (0usize, 0usize, 0usize);
        for env in envelopes.into_iter().take(MAX_PARENTS * 4) {
            let valid = env.verify().is_ok()
                && env.author == peer_pk
                && (env.kind == EnvelopeKind::Text
                    || (env.kind == EnvelopeKind::FileRef && self.limits.accepts_files))
                && env.recipients.iter().any(|r| r.recipient == for_recipient)
                && env.hlc.ms <= now.saturating_add(MAX_FUTURE_MS);
            if !valid {
                continue;
            }
            let id = env.id;
            let (acked, held) = self
                .store
                .run(move |c| {
                    Ok((
                        store::is_acked(c, &id, &for_recipient)?,
                        store::carry_has_for(c, &id, &for_recipient)?,
                    ))
                })
                .await?;
            if acked {
                continue;
            }
            if held {
                // Já guardo esta: aceitar de novo é idempotente e conta como carrier.
                eligible += 1;
                accepted.push(id);
                continue;
            }
            eligible += 1;
            let size = postcard::to_stdvec(&env)?.len() as u64;
            if total + size > self.limits.total_bytes
                || from_peer + size > self.limits.per_contact_bytes
            {
                over_quota += 1;
                continue;
            }
            let env2 = env.clone();
            let inserted = self
                .store
                .run(move |c| store::carry_insert(c, &env2, &for_recipient, &peer_pk, now, ttl_ms))
                .await?;
            if inserted {
                total += size;
                from_peer += size;
                newly += 1;
                accepted.push(env.id);
            }
        }
        stats.stored_for_others += newly;
        Ok(if !accepted.is_empty() {
            CarryMsg::Accept { ids: accepted }
        } else if over_quota > 0 {
            CarryMsg::Reject {
                reason: RejectReason::Quota,
            }
        } else if eligible == 0 {
            CarryMsg::Reject {
                reason: RejectReason::Invalid,
            }
        } else {
            CarryMsg::Accept { ids: vec![] }
        })
    }
}

/// Fica com o prefixo de `envs` que cabe em `budget` bytes serializados.
fn fit(envs: Vec<Envelope>, budget: usize) -> Result<Vec<Envelope>> {
    let mut used = 0usize;
    let mut out = Vec::new();
    for e in envs {
        let n = postcard::to_stdvec(&e)?.len();
        if used + n > budget {
            break;
        }
        used += n;
        out.push(e);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Budget, contacts::ContactStatus, Role};
    use iroh::EndpointAddr;

    struct Node {
        id: Identity,
        store: Store,
        name: &'static str,
    }

    impl Node {
        fn new(name: &'static str) -> Self {
            Self {
                id: Identity::generate(),
                store: Store::open_in_memory().unwrap(),
                name,
            }
        }

        /// Como os outros me veem.
        fn as_contact(&self) -> Contact {
            let t = self
                .id
                .invite(
                    EndpointAddr::new(self.id.endpoint_id()),
                    self.name,
                    1000,
                    3600,
                )
                .unwrap();
            Contact {
                identity_pk: t.identity_pk,
                enc_pk: t.enc_pk,
                device_cert: t.device_cert,
                node_addr: t.node_addr,
                display_name: self.name.into(),
                status: ContactStatus::Mutual,
                added_at: 0,
            }
        }

        fn pk(&self) -> [u8; 32] {
            self.id.identity_pk()
        }

        /// Mensagem de `self` para `to`, registrada como pendente de entrega.
        async fn write_to(&self, to: &Node, text: &str, ms: u64) -> Envelope {
            let c = to.as_contact();
            let env = Envelope::create(
                &self.id,
                dm_conversation_id(&self.pk(), &to.pk()),
                Hlc::new(ms, 0),
                vec![],
                &[
                    Recipient {
                        identity_pk: c.identity_pk,
                        enc_pk: c.enc_pk,
                    },
                    Recipient {
                        identity_pk: self.pk(),
                        enc_pk: self.id.enc_pk(),
                    },
                ],
                EnvelopeKind::Text,
                text.as_bytes(),
            )
            .unwrap();
            let (e, pk, tpk) = (env.clone(), self.pk(), to.pk());
            self.store
                .run(move |c| {
                    store::envelope_insert(c, &e, Some(b"x"), true, 0)?;
                    store::pending_add(c, &e.id, &tpk, 0)
                })
                .await
                .unwrap();
            let _ = pk;
            env
        }

        async fn texts_from(&self, other: &Node) -> Vec<String> {
            let conv = dm_conversation_id(&self.pk(), &other.pk());
            let all = self
                .store
                .run(move |c| store::conversation_envelopes(c, &conv))
                .await
                .unwrap();
            let mut v: Vec<_> = all
                .into_iter()
                .filter(|s| !s.outgoing)
                .map(|s| {
                    (
                        s.envelope.hlc,
                        String::from_utf8(s.plaintext.unwrap()).unwrap(),
                    )
                })
                .collect();
            v.sort();
            v.into_iter().map(|(_, t)| t).collect()
        }

        async fn carried(&self) -> u64 {
            self.store.run(|c| store::carry_count(c)).await.unwrap()
        }

        async fn pending(&self, to: &Node) -> usize {
            let tpk = to.pk();
            self.store
                .run(move |c| store::pending_list(c, &tpk))
                .await
                .unwrap()
                .len()
        }
    }

    /// Sessão entre `a` e `b`. `contacts_*` são os contatos mútuos de cada um.
    #[allow(clippy::too_many_arguments)]
    async fn session(
        a: &Node,
        a_role: Role,
        a_contacts: &[&Node],
        a_offers: Vec<(&Node, Vec<Envelope>)>,
        b: &Node,
        b_role: Role,
        b_contacts: &[&Node],
        budget: &Budget,
    ) -> (CarryStats, CarryStats) {
        let (ea, eb) = tokio::io::duplex(1 << 20);
        let (mut ra, mut wa) = tokio::io::split(ea);
        let (mut rb, mut wb) = tokio::io::split(eb);
        let (ac, bc): (Vec<Contact>, Vec<Contact>) = (
            a_contacts.iter().map(|n| n.as_contact()).collect(),
            b_contacts.iter().map(|n| n.as_contact()).collect(),
        );
        let (a_peer, b_peer) = (b.as_contact(), a.as_contact());
        let limiter = RateLimiter::new(None);
        let est = ThroughputEstimator::default();
        let tick = || Hlc::new(unix_now_ms(), 0);
        let nop = |_: &Contact, _: &Envelope| {};
        let sa = CarrySession {
            store: &a.store,
            identity: &a.id,
            peer: &a_peer,
            contacts: &ac,
            co_members: &[],
            groups: vec![],
            limits: CarryLimits::for_role(a_role, budget),
            max_frame: 1 << 20,
            limiter: &limiter,
            throughput: &est,
            tick: &tick,
            on_received: &nop,
            max_file_bytes: 1 << 30,
            on_blob_offer: &|_, _| {},
            offers: a_offers
                .into_iter()
                .map(|(n, e)| OfferSet {
                    to: n.as_contact(),
                    envelopes: e,
                    blobs: vec![],
                })
                .collect(),
        };
        let sb = CarrySession {
            store: &b.store,
            identity: &b.id,
            peer: &b_peer,
            contacts: &bc,
            co_members: &[],
            groups: vec![],
            limits: CarryLimits::for_role(b_role, budget),
            max_frame: 1 << 20,
            limiter: &limiter,
            throughput: &est,
            tick: &tick,
            on_received: &nop,
            max_file_bytes: 1 << 30,
            on_blob_offer: &|_, _| {},
            offers: vec![],
        };
        let (x, y) = tokio::join!(sa.run(&mut ra, &mut wa), sb.run(&mut rb, &mut wb));
        (x.unwrap(), y.unwrap())
    }

    #[test]
    fn month_arithmetic() {
        assert_eq!(month_of(0), 197001);
        assert_eq!(month_of(1_792_624_000_000), 202610);
        assert_eq!(month_of(951_782_400_000), 200002); // 2000-02-29 (ano bissexto)
        assert_eq!(month_of(951_868_800_000), 200003); // 2000-03-01
        assert_eq!(month_of(1_704_067_199_000), 202312);
        assert_eq!(month_of(1_704_067_200_000), 202401);
    }

    #[tokio::test]
    async fn offer_is_accepted_and_stored_for_the_recipient() {
        let (a, b, c) = (Node::new("a"), Node::new("b"), Node::new("c"));
        let env = a.write_to(&b, "oi b", 10).await;
        let budget = Budget::default();
        let (sa, sc) = session(
            &a,
            Role::Peer,
            &[&b, &c],
            vec![(&b, vec![env.clone()])],
            &c,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!(sa.accepted, vec![(b.pk(), vec![env.id])]);
        assert_eq!(sc.stored_for_others, 1);
        assert_eq!(c.carried().await, 1);
        // Oferta repetida não duplica nem estoura nada.
        let (sa2, _) = session(
            &a,
            Role::Peer,
            &[&b, &c],
            vec![(&b, vec![env.clone()])],
            &c,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!(
            sa2.accepted,
            vec![(b.pk(), vec![env.id])],
            "aceitar de novo é idempotente"
        );
        assert_eq!(c.carried().await, 1, "sem duplicar");
        // O carrier ficou registrado para A.
        let id = env.id;
        let carriers = a
            .store
            .run(move |conn| store::offered_carriers(conn, &id))
            .await
            .unwrap();
        assert_eq!(carriers, vec![c.pk()]);
    }

    #[tokio::test]
    async fn leaf_refuses_and_leafplus_has_a_small_quota() {
        let (a, b, c) = (Node::new("a"), Node::new("b"), Node::new("c"));
        let env = a.write_to(&b, "oi", 10).await;
        let budget = Budget::default();
        let (sa, _) = session(
            &a,
            Role::Peer,
            &[&b, &c],
            vec![(&b, vec![env.clone()])],
            &c,
            Role::Leaf,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!(sa.rejected, vec![RejectReason::NotCarrying]);
        assert_eq!(c.carried().await, 0);
        // Quota mínima de 0 MB: recusa por quota.
        let tiny = Budget {
            carry_quota_mb_total: Some(0),
            carry_quota_mb_per_contact: Some(0),
            ..Budget::default()
        };
        let (sa, _) = session(
            &a,
            Role::LeafPlus,
            &[&b, &c],
            vec![(&b, vec![env.clone()])],
            &c,
            Role::LeafPlus,
            &[&a, &b],
            &tiny,
        )
        .await;
        assert_eq!(sa.rejected, vec![RejectReason::Quota]);
        assert_eq!(c.carried().await, 0);
        // Com a quota padrão do LeafPlus, aceita texto.
        let (sa, _) = session(
            &a,
            Role::LeafPlus,
            &[&b, &c],
            vec![(&b, vec![env])],
            &c,
            Role::LeafPlus,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!(sa.accepted.len(), 1);
        assert_eq!(c.carried().await, 1);
    }

    fn fileref_env(from: &Node, to: &Node, blob: u8) -> Envelope {
        let fr = crate::FileRef {
            blob_hash: [blob; 32],
            size: 1000,
            name: "foto.png".into(),
            mime: "image/png".into(),
            file_key: [3; 32],
        };
        let c = to.as_contact();
        Envelope::create(
            &from.id,
            dm_conversation_id(&from.pk(), &to.pk()),
            Hlc::new(5, 0),
            vec![],
            &[Recipient {
                identity_pk: c.identity_pk,
                enc_pk: c.enc_pk,
            }],
            EnvelopeKind::FileRef,
            &fr.encode().unwrap(),
        )
        .unwrap()
    }

    /// Roda uma oferta `A → C` (para `B`) e devolve o que `C` recebeu como pedido de blobs.
    async fn offer_file(c_role: Role) -> (CarryStats, Vec<BlobOffer>, u64) {
        let (a, b, c) = (Node::new("a"), Node::new("b"), Node::new("c"));
        let env = fileref_env(&a, &b, 9);
        let (ea, eb) = tokio::io::duplex(1 << 20);
        let (mut ra, mut wa) = tokio::io::split(ea);
        let (mut rb, mut wb) = tokio::io::split(eb);
        let (ac, cc) = (
            vec![b.as_contact(), c.as_contact()],
            vec![a.as_contact(), b.as_contact()],
        );
        let (c_peer, a_peer) = (c.as_contact(), a.as_contact());
        let budget = Budget::default();
        let (limiter, est) = (RateLimiter::new(None), ThroughputEstimator::default());
        let tick = || Hlc::new(unix_now_ms(), 0);
        let nop = |_: &Contact, _: &Envelope| {};
        let asked = std::sync::Mutex::new(Vec::new());
        let on_blob = |_: &Contact, blobs: Vec<BlobOffer>| asked.lock().unwrap().extend(blobs);
        let sa = CarrySession {
            store: &a.store,
            identity: &a.id,
            peer: &c_peer,
            contacts: &ac,
            co_members: &[],
            groups: vec![],
            limits: CarryLimits::for_role(Role::Peer, &budget),
            max_frame: 1 << 20,
            limiter: &limiter,
            throughput: &est,
            tick: &tick,
            on_received: &nop,
            max_file_bytes: 1 << 30,
            on_blob_offer: &|_, _| {},
            offers: vec![OfferSet {
                to: b.as_contact(),
                envelopes: vec![env],
                blobs: vec![BlobOffer {
                    hash: [9; 32],
                    enc_size: 1100,
                }],
            }],
        };
        let sc = CarrySession {
            store: &c.store,
            identity: &c.id,
            peer: &a_peer,
            contacts: &cc,
            co_members: &[],
            groups: vec![],
            limits: CarryLimits::for_role(c_role, &budget),
            max_frame: 1 << 20,
            limiter: &limiter,
            throughput: &est,
            tick: &tick,
            on_received: &nop,
            max_file_bytes: 1 << 30,
            on_blob_offer: &on_blob,
            offers: vec![],
        };
        let (x, _) = tokio::join!(sa.run(&mut ra, &mut wa), sc.run(&mut rb, &mut wb));
        let held = c.carried().await;
        let asked = asked.into_inner().unwrap();
        (x.unwrap(), asked, held)
    }

    #[tokio::test]
    async fn file_messages_are_only_carried_by_relay_and_super_peer_and_trigger_a_blob_fetch() {
        // Peer e LeafPlus recusam envelopes FileRef (só texto); não pedem o blob.
        for weak in [Role::LeafPlus, Role::Peer] {
            let (stats, asked, held) = offer_file(weak).await;
            assert_eq!(stats.rejected, vec![RejectReason::Invalid], "{weak:?}");
            assert!(asked.is_empty() && held == 0, "{weak:?}");
        }
        // Relay e SuperPeer aceitam e pedem a replicação do blob oferecido.
        for strong in [Role::Relay, Role::SuperPeer] {
            let (stats, asked, held) = offer_file(strong).await;
            assert_eq!(stats.accepted.len(), 1, "{strong:?}");
            assert_eq!(held, 1, "{strong:?}");
            assert_eq!(
                asked,
                vec![BlobOffer {
                    hash: [9; 32],
                    enc_size: 1100
                }],
                "{strong:?}"
            );
        }
    }

    #[tokio::test]
    async fn offers_for_non_contacts_or_forged_envelopes_are_rejected() {
        let (a, b, c, stranger) = (
            Node::new("a"),
            Node::new("b"),
            Node::new("c"),
            Node::new("s"),
        );
        let budget = Budget::default();
        // Destinatário que C não conhece.
        let env = a.write_to(&stranger, "x", 10).await;
        let (sa, _) = session(
            &a,
            Role::Peer,
            &[&stranger, &c],
            vec![(&stranger, vec![env])],
            &c,
            Role::Peer,
            &[&a],
            &budget,
        )
        .await;
        assert_eq!(sa.rejected, vec![RejectReason::NotContact]);
        // Envelope adulterado.
        let mut bad = a.write_to(&b, "y", 11).await;
        bad.ciphertext[30] ^= 1;
        let (sa, _) = session(
            &a,
            Role::Peer,
            &[&b, &c],
            vec![(&b, vec![bad])],
            &c,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!(sa.rejected, vec![RejectReason::Invalid]);
        // Envelope de outro autor (B tenta empurrar mensagem escrita por A).
        let others = a.write_to(&b, "z", 12).await;
        let (sb, _) = session(
            &b,
            Role::Peer,
            &[&a, &c],
            vec![(&a, vec![others])],
            &c,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!(sb.rejected, vec![RejectReason::Invalid]);
        assert_eq!(c.carried().await, 0);
    }

    #[tokio::test]
    async fn carrier_delivers_to_the_recipient_and_the_receipt_purges_the_copy() {
        let (a, b, c) = (Node::new("a"), Node::new("b"), Node::new("c"));
        let budget = Budget::default();
        let env = a.write_to(&b, "chegou via C", 10).await;
        // A → C: oferta aceita.
        session(
            &a,
            Role::Peer,
            &[&b, &c],
            vec![(&b, vec![env.clone()])],
            &c,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!(c.carried().await, 1);
        assert_eq!(a.pending(&b).await, 1);
        // C encontra B: entrega.
        let (sc, sb) = session(
            &c,
            Role::Peer,
            &[&a, &b],
            vec![],
            &b,
            Role::Peer,
            &[&a, &c],
            &budget,
        )
        .await;
        assert_eq!(sc.delivered, 1);
        assert_eq!(sb.received, 1);
        assert_eq!(b.texts_from(&a).await, vec!["chegou via C"]);
        // O recibo de B voltou para C na mesma sessão e expurgou a cópia.
        assert_eq!(sc.purged, 1, "{sc:?}");
        assert_eq!(c.carried().await, 0);
        // Segunda sessão: nada a entregar nem duplicar.
        let (sc, sb) = session(
            &c,
            Role::Peer,
            &[&a, &b],
            vec![],
            &b,
            Role::Peer,
            &[&a, &c],
            &budget,
        )
        .await;
        assert_eq!((sc.delivered, sb.received), (0, 0));
        // O recibo de B chega a A pela carona do C: A deixa de ter a mensagem como pendente.
        let (_, sa) = session(
            &c,
            Role::Peer,
            &[&a, &b],
            vec![],
            &a,
            Role::Peer,
            &[&b, &c],
            &budget,
        )
        .await;
        assert_eq!(sa.receipts_learned, 1);
        assert_eq!(a.pending(&b).await, 0);
    }

    #[tokio::test]
    async fn a_receipt_purges_every_redundant_copy() {
        let (a, b, c, d) = (
            Node::new("a"),
            Node::new("b"),
            Node::new("c"),
            Node::new("d"),
        );
        let budget = Budget::default();
        let env = a.write_to(&b, "duas cópias", 10).await;
        session(
            &a,
            Role::Peer,
            &[&b, &c, &d],
            vec![(&b, vec![env.clone()])],
            &c,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        session(
            &a,
            Role::Peer,
            &[&b, &c, &d],
            vec![(&b, vec![env.clone()])],
            &d,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!((c.carried().await, d.carried().await), (1, 1));
        // B recebe por C.
        session(
            &c,
            Role::Peer,
            &[&a, &b],
            vec![],
            &b,
            Role::Peer,
            &[&a, &c, &d],
            &budget,
        )
        .await;
        assert_eq!(c.carried().await, 0);
        assert_eq!(d.carried().await, 1);
        // B encontra D: já tem a mensagem (duplicada), mas reconhece e o recibo expurga a cópia.
        let (sd, sb) = session(
            &d,
            Role::Peer,
            &[&a, &b],
            vec![],
            &b,
            Role::Peer,
            &[&a, &c, &d],
            &budget,
        )
        .await;
        assert_eq!(sb.received, 0, "duplicada");
        assert_eq!(sd.purged, 1);
        assert_eq!(d.carried().await, 0);
        assert_eq!(b.texts_from(&a).await, vec!["duas cópias"]);
    }

    #[tokio::test]
    async fn already_acked_messages_are_not_carried_again() {
        let (a, b, c) = (Node::new("a"), Node::new("b"), Node::new("c"));
        let budget = Budget::default();
        let env = a.write_to(&b, "velha", 10).await;
        session(
            &a,
            Role::Peer,
            &[&b, &c],
            vec![(&b, vec![env.clone()])],
            &c,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        session(
            &c,
            Role::Peer,
            &[&a, &b],
            vec![],
            &b,
            Role::Peer,
            &[&a, &c],
            &budget,
        )
        .await;
        // C aprendeu o recibo; uma oferta tardia da mesma mensagem não é guardada de novo.
        let (sa, _) = session(
            &a,
            Role::Peer,
            &[&b, &c],
            vec![(&b, vec![env])],
            &c,
            Role::Peer,
            &[&a, &b],
            &budget,
        )
        .await;
        assert_eq!(sa.rejected, vec![RejectReason::Invalid]);
        assert_eq!(c.carried().await, 0);
    }

    #[tokio::test]
    async fn delivery_from_an_unknown_author_is_dropped_without_a_receipt() {
        let (a, b, c, stranger) = (
            Node::new("a"),
            Node::new("b"),
            Node::new("c"),
            Node::new("s"),
        );
        let budget = Budget::default();
        // C carrega algo escrito por quem B não conhece.
        let env = stranger.write_to(&b, "intruso", 10).await;
        let e2 = env.clone();
        let bpk = b.pk();
        c.store
            .run(move |conn| store::carry_insert(conn, &e2, &bpk, &[9; 32], 0, 1_000_000_000_000))
            .await
            .unwrap();
        let (_, sb) = session(
            &c,
            Role::Peer,
            &[&b],
            vec![],
            &b,
            Role::Peer,
            &[&a, &c],
            &budget,
        )
        .await;
        assert_eq!(sb.received, 0);
        assert!(b.texts_from(&stranger).await.is_empty());
        let _ = a;
    }

    #[tokio::test]
    async fn forged_receipts_do_not_purge_anything() {
        let (a, b, c, mallory) = (
            Node::new("a"),
            Node::new("b"),
            Node::new("c"),
            Node::new("m"),
        );
        let budget = Budget::default();
        let env = a.write_to(&b, "protegida", 10).await;
        session(
            &a,
            Role::Peer,
            &[&b, &c],
            vec![(&b, vec![env.clone()])],
            &c,
            Role::Peer,
            &[&a, &b, &mallory],
            &budget,
        )
        .await;
        assert_eq!(c.carried().await, 1);
        // Mallory (contato de C) fabrica um "recibo" em nome próprio para o id.
        let fake = make_receipt(&mallory.id, &b.as_contact(), &[env.id], Hlc::new(20, 0)).unwrap();
        let (f2, now) = (fake.clone(), unix_now_ms());
        mallory
            .store
            .run(move |conn| store::receipt_store(conn, &f2, now, 1_000_000))
            .await
            .unwrap();
        let (_, _) = session(
            &mallory,
            Role::Peer,
            &[&c],
            vec![],
            &c,
            Role::Peer,
            &[&a, &b, &mallory],
            &budget,
        )
        .await;
        // O recibo de Mallory vale só para entregas *a Mallory*: não expurga a cópia de B.
        assert_eq!(c.carried().await, 1);
    }
}
