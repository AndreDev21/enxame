//! Sinalização de chamadas (Fase 6.4, ADR 0012): envelopes `CallSignal` E2E sobre `enxame/call/1`.
//!
//! São mensagens de tempo real (oferta/resposta SDP, candidatos ICE, desligar): não entram no
//! histórico, não ficam pendentes e não passam por carriers. Só contatos mútuos, só mensagens
//! frescas e sem repetição.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{
    contacts::Contact,
    crypto::{dm_conversation_id, Envelope, EnvelopeKind},
    identity::Identity,
    Error, Result,
};

/// Maior carga de um sinal (SDP com vídeo cabe com folga).
pub const MAX_SIGNAL_BYTES: usize = 64 * 1024;
/// Um sinal com carimbo mais distante que isto do meu relógio é descartado. É o mesmo limite das
/// mensagens (24 h): PCs com fuso ou hora errados são comuns (no campo, um Windows 3 h fora do ar) e
/// a proteção contra repetição vem do id inédito e do canal cifrado, não do relógio. O carimbo é o
/// relógio de parede de quem envia, não o HLC, que pode andar adiantado.
pub const MAX_SKEW: Duration = Duration::from_secs(24 * 3600);
/// Quantos ids de sinais recentes lembrar para recusar repetição (os mais antigos saem primeiro).
const SEEN_CAP: usize = 10_000;
/// Sinais aceitos por contato numa janela de 10 s (uma chamada usa algumas dezenas).
const RATE_LIMIT: usize = 200;
const RATE_WINDOW: Duration = Duration::from_secs(10);

/// Memória curta para recusar repetições e limitar a taxa.
#[derive(Debug, Default)]
pub struct SignalGuard {
    seen: std::collections::HashSet<[u8; 32]>,
    /// Ordem de chegada dos ids de `seen`, para descartar os mais antigos ao passar de `SEEN_CAP`.
    order: std::collections::VecDeque<[u8; 32]>,
    rate: HashMap<[u8; 32], Vec<Instant>>,
}

impl SignalGuard {
    /// Registra o sinal ou devolve o motivo da recusa.
    fn admit(&mut self, id: [u8; 32], from: [u8; 32], now: Instant) -> Result<()> {
        if self.seen.contains(&id) {
            return Err(Error::Protocol("sinal de chamada repetido".into()));
        }
        let window = self.rate.entry(from).or_default();
        window.retain(|t| now.duration_since(*t) < RATE_WINDOW);
        if window.len() >= RATE_LIMIT {
            return Err(Error::Protocol("sinais de chamada demais".into()));
        }
        window.push(now);
        self.seen.insert(id);
        self.order.push_back(id);
        while self.order.len() > SEEN_CAP {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        Ok(())
    }
}

/// Cria o envelope de um sinal para `to`: selado só para ele e assinado por mim.
pub fn make_signal(
    identity: &Identity,
    to: &Contact,
    hlc: crate::hlc::Hlc,
    payload: &[u8],
) -> Result<Envelope> {
    if payload.is_empty() || payload.len() > MAX_SIGNAL_BYTES {
        return Err(Error::Contact(
            "sinal de chamada vazio ou grande demais".into(),
        ));
    }
    Envelope::create(
        identity,
        dm_conversation_id(&identity.identity_pk(), &to.identity_pk),
        hlc,
        vec![],
        &[crate::crypto::Recipient {
            identity_pk: to.identity_pk,
            enc_pk: to.enc_pk,
        }],
        EnvelopeKind::CallSignal,
        payload,
    )
}

/// Valida e abre um sinal recebido de `from` (contato mútuo). Devolve a carga.
pub fn open_signal(
    identity: &Identity,
    from: &Contact,
    env: &Envelope,
    now_ms: u64,
    guard: &mut SignalGuard,
) -> Result<Vec<u8>> {
    env.verify()?;
    if env.kind != EnvelopeKind::CallSignal {
        return Err(Error::Protocol("não é um sinal de chamada".into()));
    }
    if env.author != from.identity_pk {
        return Err(Error::Protocol("sinal de chamada de outro autor".into()));
    }
    if env.conversation_id != dm_conversation_id(&identity.identity_pk(), &from.identity_pk) {
        return Err(Error::Protocol(
            "sinal de chamada em conversa alheia".into(),
        ));
    }
    if env.recipients.len() != 1 || !env.parents.is_empty() {
        return Err(Error::Protocol("sinal de chamada malformado".into()));
    }
    let skew = now_ms.abs_diff(env.hlc.ms);
    if skew > MAX_SKEW.as_millis() as u64 {
        return Err(Error::Protocol(format!(
            "relógio fora de hora: {} s de diferença entre os computadores (limite {} s)",
            skew / 1000,
            MAX_SKEW.as_secs()
        )));
    }
    let plain = env.open(identity)?;
    if plain.is_empty() || plain.len() > MAX_SIGNAL_BYTES {
        return Err(Error::Protocol("carga de sinal inválida".into()));
    }
    guard.admit(env.id, env.author, Instant::now())?;
    Ok(plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contacts::ContactStatus, hlc::Hlc, identity::unix_now_ms};
    use iroh::EndpointAddr;

    fn contact_of(id: &Identity) -> Contact {
        Contact {
            identity_pk: id.identity_pk(),
            enc_pk: id.enc_pk(),
            device_cert: id.device_cert(1),
            node_addr: EndpointAddr::new(id.endpoint_id()),
            display_name: "x".into(),
            status: ContactStatus::Mutual,
            added_at: 0,
        }
    }

    fn setup() -> (Identity, Identity, Contact, Contact) {
        let (a, b) = (Identity::generate(), Identity::generate());
        let (ca, cb) = (contact_of(&a), contact_of(&b));
        (a, b, ca, cb)
    }

    #[test]
    fn a_fresh_signal_opens_once() {
        let (a, b, ca, cb) = setup();
        let now = unix_now_ms();
        let env = make_signal(&a, &cb, Hlc::new(now, 0), b"{\"t\":\"offer\"}").unwrap();
        let mut g = SignalGuard::default();
        assert_eq!(
            open_signal(&b, &ca, &env, now, &mut g).unwrap(),
            b"{\"t\":\"offer\"}"
        );
        // O mesmo envelope de novo é repetição.
        assert!(open_signal(&b, &ca, &env, now, &mut g).is_err());
    }

    #[test]
    fn stale_future_forged_and_foreign_signals_are_rejected() {
        let (a, b, ca, cb) = setup();
        let c = Identity::generate();
        let now = unix_now_ms();
        let mut g = SignalGuard::default();
        // Velho e do futuro.
        let old = make_signal(&a, &cb, Hlc::new(now - 25 * 3600 * 1000, 0), b"x").unwrap();
        assert!(open_signal(&b, &ca, &old, now, &mut g).is_err());
        let future = make_signal(&a, &cb, Hlc::new(now + 25 * 3600 * 1000, 0), b"x").unwrap();
        assert!(open_signal(&b, &ca, &future, now, &mut g).is_err());
        // Autor diferente do contato de onde veio.
        let from_c = make_signal(&c, &cb, Hlc::new(now, 0), b"x").unwrap();
        assert!(open_signal(&b, &ca, &from_c, now, &mut g).is_err());
        // Adulterado: o id deixa de bater.
        let mut tampered = make_signal(&a, &cb, Hlc::new(now, 1), b"x").unwrap();
        tampered.ciphertext[30] ^= 1;
        assert!(open_signal(&b, &ca, &tampered, now, &mut g).is_err());
        // Tipo errado (texto comum não é sinal).
        let text = Envelope::create(
            &a,
            dm_conversation_id(&a.identity_pk(), &b.identity_pk()),
            Hlc::new(now, 2),
            vec![],
            &[crate::crypto::Recipient {
                identity_pk: b.identity_pk(),
                enc_pk: b.enc_pk(),
            }],
            EnvelopeKind::Text,
            b"oi",
        )
        .unwrap();
        assert!(open_signal(&b, &ca, &text, now, &mut g).is_err());
        // Selado para outra pessoa: não consigo abrir.
        let for_c = make_signal(&a, &contact_of(&c), Hlc::new(now, 3), b"x").unwrap();
        assert!(open_signal(&b, &ca, &for_c, now, &mut g).is_err());
    }

    /// No campo: um Windows com o fuso errado (relógio 3 h fora) não pode ser recusado.
    #[test]
    fn a_pc_with_the_clock_three_hours_off_still_gets_through() {
        let (a, b, ca, cb) = setup();
        let now = unix_now_ms();
        let mut g = SignalGuard::default();
        for (i, skew) in [-3i64 * 3600 * 1000, 3 * 3600 * 1000, 23 * 3600 * 1000]
            .into_iter()
            .enumerate()
        {
            let env = make_signal(
                &a,
                &cb,
                Hlc::new((now as i64 + skew) as u64, i as u32),
                b"x",
            )
            .unwrap();
            assert!(
                open_signal(&b, &ca, &env, now, &mut g).is_ok(),
                "skew {skew} ms"
            );
        }
    }

    #[test]
    fn the_replay_memory_is_bounded() {
        let mut g = SignalGuard::default();
        let t = Instant::now();
        let from = [9u8; 32];
        for i in 0..(SEEN_CAP + 50) as u32 {
            let mut id = [0u8; 32];
            id[..4].copy_from_slice(&i.to_le_bytes());
            // Contatos diferentes para não esbarrar no limite de taxa.
            let mut f = from;
            f[..4].copy_from_slice(&i.to_le_bytes());
            g.admit(id, f, t).unwrap();
        }
        assert_eq!(g.seen.len(), SEEN_CAP);
        // O mais antigo foi esquecido (pode repetir); o mais novo continua lembrado.
        let mut oldest = [0u8; 32];
        oldest[..4].copy_from_slice(&0u32.to_le_bytes());
        assert!(g.admit(oldest, [1u8; 32], t).is_ok());
        let mut newest = [0u8; 32];
        newest[..4].copy_from_slice(&((SEEN_CAP + 49) as u32).to_le_bytes());
        assert!(g.admit(newest, [2u8; 32], t).is_err());
    }

    #[test]
    fn payload_limits_and_rate_limit() {
        let (a, b, ca, cb) = setup();
        assert!(make_signal(&a, &cb, Hlc::new(1, 0), b"").is_err());
        assert!(make_signal(&a, &cb, Hlc::new(1, 0), &vec![0u8; MAX_SIGNAL_BYTES + 1]).is_err());
        let now = unix_now_ms();
        let mut g = SignalGuard::default();
        let mut refused = 0;
        for i in 0..(RATE_LIMIT as u32 + 20) {
            let env = make_signal(&a, &cb, Hlc::new(now, i), b"ice").unwrap();
            if open_signal(&b, &ca, &env, now, &mut g).is_err() {
                refused += 1;
            }
        }
        assert_eq!(refused, 20, "acima do limite por janela é recusado");
    }
}
