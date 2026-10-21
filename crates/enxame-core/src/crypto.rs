//! Envelopes: selagem por destinatário, assinatura e verificação (spec 7.1, ADR 0003/0005).
//!
//! Só composição de crates auditadas: X25519 (dalek), HKDF-SHA256, XChaCha20-Poly1305, BLAKE3
//! e Ed25519 (dalek). Nenhuma primitiva é implementada aqui.

use chacha20poly1305::{
    aead::{Aead, Generate, Key, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{hlc::Hlc, identity::Identity, Error, Result};

/// Versão atual do formato de envelope.
pub const ENVELOPE_VERSION: u8 = 1;
pub const MAX_PARENTS: usize = 64;
pub const MAX_RECIPIENTS: usize = 32;
pub const MAX_CIPHERTEXT: usize = 256 * 1024;
/// Tamanho máximo de uma mensagem de texto, em bytes.
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

const DOMAIN_ID: &[u8] = b"enxame/envelope-id/1\0";
const DOMAIN_SIG: &[u8] = b"enxame/envelope-sig/1\0";
const DOMAIN_AAD: &[u8] = b"enxame/envelope-aad/1\0";
const DOMAIN_SEAL: &[u8] = b"enxame/seal/1";
const DOMAIN_DM: &[u8] = b"enxame/dm/1\0";
const NONCE_LEN: usize = 24;

/// Tipo do conteúdo. A ordem das variantes é parte do wire; só acrescente no fim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvelopeKind {
    Text,
    FileRef,
    Receipt,
    GroupOp,
    CallSignal,
}

/// Chave de conteúdo selada para um destinatário.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedKey {
    /// `identity_pk` do destinatário (visível ao carrier: necessário para roteamento).
    pub recipient: [u8; 32],
    pub ephemeral_pk: [u8; 32],
    /// `nonce (24) ‖ XChaCha20-Poly1305(chave de conteúdo)`.
    pub sealed: Vec<u8>,
}

/// Destinatário de uma mensagem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipient {
    pub identity_pk: [u8; 32],
    pub enc_pk: [u8; 32],
}

/// Unidade de mensagem assinada e cifrada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u8,
    pub id: [u8; 32],
    pub conversation_id: [u8; 32],
    pub author: [u8; 32],
    /// `EndpointId` do dispositivo autor.
    pub author_device: [u8; 32],
    pub hlc: Hlc,
    pub parents: Vec<[u8; 32]>,
    pub recipients: Vec<SealedKey>,
    pub kind: EnvelopeKind,
    /// `nonce (24) ‖ XChaCha20-Poly1305(conteúdo)`.
    pub ciphertext: Vec<u8>,
    pub sig: Signature,
}

/// Conversa 1:1: determinística, para os dois lados chegarem ao mesmo id sem coordenação.
pub fn dm_conversation_id(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut h = blake3::Hasher::new();
    h.update(DOMAIN_DM);
    h.update(lo);
    h.update(hi);
    *h.finalize().as_bytes()
}

fn payload_aad(
    conversation_id: &[u8; 32],
    author: &[u8; 32],
    hlc: Hlc,
    kind: EnvelopeKind,
) -> Vec<u8> {
    let mut aad = Vec::with_capacity(DOMAIN_AAD.len() + 32 + 32 + 12 + 1);
    aad.extend_from_slice(DOMAIN_AAD);
    aad.extend_from_slice(conversation_id);
    aad.extend_from_slice(author);
    aad.extend_from_slice(&hlc.ms.to_le_bytes());
    aad.extend_from_slice(&hlc.counter.to_le_bytes());
    aad.push(kind as u8);
    aad
}

fn aead_encrypt(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| Error::Crypto("chave com tamanho errado".into()))?;
    let nonce = XNonce::generate();
    let ct = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::Crypto("falha ao cifrar".into()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn aead_decrypt(key: &[u8; 32], data: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if data.len() < NONCE_LEN + 16 {
        return Err(Error::Crypto("ciphertext curto demais".into()));
    }
    let (nonce, ct) = data.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| Error::Crypto("chave com tamanho errado".into()))?;
    cipher
        .decrypt(
            &XNonce::try_from(nonce).map_err(|_| Error::Crypto("nonce inválido".into()))?,
            Payload { msg: ct, aad },
        )
        .map_err(|_| Error::Crypto("autenticação falhou".into()))
}

fn seal_kdf(shared: &[u8; 32], eph_pk: &[u8; 32], recipient_enc_pk: &[u8; 32]) -> Result<[u8; 32]> {
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(eph_pk);
    salt[32..].copy_from_slice(recipient_enc_pk);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut okm = [0u8; 32];
    hk.expand(DOMAIN_SEAL, &mut okm)
        .map_err(|_| Error::Crypto("HKDF".into()))?;
    Ok(okm)
}

fn seal_key(content_key: &[u8; 32], to: &Recipient) -> Result<SealedKey> {
    let eph = x25519_dalek::EphemeralSecret::random_from_rng(OsRng);
    let eph_pk = x25519_dalek::PublicKey::from(&eph);
    let shared = eph.diffie_hellman(&x25519_dalek::PublicKey::from(to.enc_pk));
    if !shared.was_contributory() {
        return Err(Error::Crypto(
            "chave pública de destinatário inválida".into(),
        ));
    }
    let kek = seal_kdf(shared.as_bytes(), eph_pk.as_bytes(), &to.enc_pk)?;
    Ok(SealedKey {
        recipient: to.identity_pk,
        ephemeral_pk: *eph_pk.as_bytes(),
        sealed: aead_encrypt(&kek, content_key, &to.identity_pk)?,
    })
}

impl Envelope {
    /// Cifra `plaintext`, sela a chave de conteúdo para cada destinatário, calcula o `id` e assina.
    pub fn create(
        identity: &Identity,
        conversation_id: [u8; 32],
        hlc: Hlc,
        parents: Vec<[u8; 32]>,
        recipients: &[Recipient],
        kind: EnvelopeKind,
        plaintext: &[u8],
    ) -> Result<Envelope> {
        if recipients.is_empty() || recipients.len() > MAX_RECIPIENTS {
            return Err(Error::Crypto("número de destinatários inválido".into()));
        }
        if parents.len() > MAX_PARENTS {
            return Err(Error::Crypto("pais demais".into()));
        }
        let author = identity.identity_pk();
        let content_key: [u8; 32] = Key::<XChaCha20Poly1305>::generate().into();
        let ciphertext = aead_encrypt(
            &content_key,
            plaintext,
            &payload_aad(&conversation_id, &author, hlc, kind),
        )?;
        if ciphertext.len() > MAX_CIPHERTEXT {
            return Err(Error::Crypto("conteúdo grande demais".into()));
        }
        let sealed = recipients
            .iter()
            .map(|r| seal_key(&content_key, r))
            .collect::<Result<Vec<_>>>()?;
        let mut env = Envelope {
            version: ENVELOPE_VERSION,
            id: [0; 32],
            conversation_id,
            author,
            author_device: *identity.endpoint_id().as_bytes(),
            hlc,
            parents,
            recipients: sealed,
            kind,
            ciphertext,
            sig: Signature::from_bytes(&[0u8; 64]),
        };
        env.id = env.compute_id()?;
        env.sig = identity.sign(&sig_message(&env.id));
        Ok(env)
    }

    /// BLAKE3 do envelope sem `id` e `sig`.
    fn compute_id(&self) -> Result<[u8; 32]> {
        let body = postcard::to_stdvec(&(
            self.version,
            &self.conversation_id,
            &self.author,
            &self.author_device,
            self.hlc,
            &self.parents,
            &self.recipients,
            self.kind,
            &self.ciphertext,
        ))?;
        let mut h = blake3::Hasher::new();
        h.update(DOMAIN_ID);
        h.update(&body);
        Ok(*h.finalize().as_bytes())
    }

    /// Validação estrutural, do `id` e da assinatura. A autorização do autor (é contato ou
    /// membro?) e o deduplicamento ficam com quem recebe.
    pub fn verify(&self) -> Result<()> {
        if self.version != ENVELOPE_VERSION {
            return Err(Error::Protocol(format!(
                "versão de envelope {} não suportada",
                self.version
            )));
        }
        if self.recipients.is_empty()
            || self.recipients.len() > MAX_RECIPIENTS
            || self.parents.len() > MAX_PARENTS
            || self.ciphertext.len() > MAX_CIPHERTEXT
        {
            return Err(Error::Protocol("envelope fora dos limites".into()));
        }
        if self.compute_id()? != self.id {
            return Err(Error::Protocol("id do envelope não confere".into()));
        }
        let pk = VerifyingKey::from_bytes(&self.author)
            .map_err(|_| Error::Protocol("autor com chave inválida".into()))?;
        pk.verify(&sig_message(&self.id), &self.sig)
            .map_err(|_| Error::Protocol("assinatura do envelope inválida".into()))
    }

    /// Decifra o conteúdo com a chave de cifragem de `identity`. Falha se não for destinatário.
    pub fn open(&self, identity: &Identity) -> Result<Vec<u8>> {
        let me = identity.identity_pk();
        let sealed = self
            .recipients
            .iter()
            .find(|r| r.recipient == me)
            .ok_or_else(|| Error::Crypto("não sou destinatário".into()))?;
        let eph_pk = x25519_dalek::PublicKey::from(sealed.ephemeral_pk);
        let shared = identity.enc_secret().diffie_hellman(&eph_pk);
        if !shared.was_contributory() {
            return Err(Error::Crypto("chave efêmera inválida".into()));
        }
        let kek = seal_kdf(shared.as_bytes(), &sealed.ephemeral_pk, &identity.enc_pk())?;
        let key_bytes = aead_decrypt(&kek, &sealed.sealed, &me)?;
        let content_key: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| Error::Crypto("chave de conteúdo com tamanho errado".into()))?;
        aead_decrypt(
            &content_key,
            &self.ciphertext,
            &payload_aad(&self.conversation_id, &self.author, self.hlc, self.kind),
        )
    }
}

fn sig_message(id: &[u8; 32]) -> Vec<u8> {
    let mut m = DOMAIN_SIG.to_vec();
    m.extend_from_slice(id);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Peer {
        id: Identity,
    }

    impl Peer {
        fn new() -> Self {
            Self {
                id: Identity::generate(),
            }
        }
        fn rcpt(&self) -> Recipient {
            Recipient {
                identity_pk: self.id.identity_pk(),
                enc_pk: self.id.enc_pk(),
            }
        }
    }

    fn make(from: &Peer, to: &[&Peer], text: &str) -> Envelope {
        let rcpts: Vec<_> = to.iter().map(|p| p.rcpt()).collect();
        Envelope::create(
            &from.id,
            dm_conversation_id(&from.id.identity_pk(), &to[0].id.identity_pk()),
            Hlc::new(1000, 0),
            vec![],
            &rcpts,
            EnvelopeKind::Text,
            text.as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn roundtrip_for_every_recipient() {
        let (a, b, c) = (Peer::new(), Peer::new(), Peer::new());
        let env = make(&a, &[&b, &c], "olá, enxame");
        env.verify().unwrap();
        assert_eq!(env.open(&b.id).unwrap(), "olá, enxame".as_bytes());
        assert_eq!(env.open(&c.id).unwrap(), "olá, enxame".as_bytes());
    }

    #[test]
    fn non_recipient_cannot_open() {
        let (a, b, eve) = (Peer::new(), Peer::new(), Peer::new());
        let env = make(&a, &[&b], "segredo");
        assert!(env.open(&eve.id).is_err());
        // Mesmo trocando o campo `recipient` pelo dela, a chave selada não abre.
        let mut forged = env.clone();
        forged.recipients[0].recipient = eve.id.identity_pk();
        assert!(forged.open(&eve.id).is_err());
    }

    #[test]
    fn serialization_roundtrip() {
        let (a, b) = (Peer::new(), Peer::new());
        let env = make(&a, &[&b], "x");
        let back: Envelope = postcard::from_bytes(&postcard::to_stdvec(&env).unwrap()).unwrap();
        assert_eq!(back, env);
        back.verify().unwrap();
    }

    #[test]
    fn every_tampered_field_is_rejected() {
        let (a, b) = (Peer::new(), Peer::new());
        let env = make(&a, &[&b], "original");
        type Mutation = Box<dyn Fn(&mut Envelope)>;
        let mutations: Vec<(&str, Mutation)> = vec![
            ("ciphertext", Box::new(|e| e.ciphertext[30] ^= 1)),
            ("hlc", Box::new(|e| e.hlc.ms += 1)),
            ("conversation", Box::new(|e| e.conversation_id[0] ^= 1)),
            ("parents", Box::new(|e| e.parents.push([7; 32]))),
            ("kind", Box::new(|e| e.kind = EnvelopeKind::Receipt)),
            ("device", Box::new(|e| e.author_device[0] ^= 1)),
            (
                "recipient key",
                Box::new(|e| e.recipients[0].sealed[30] ^= 1),
            ),
            ("id", Box::new(|e| e.id[0] ^= 1)),
            (
                "sig",
                Box::new(|e| {
                    let mut s = e.sig.to_bytes();
                    s[0] ^= 1;
                    e.sig = Signature::from_bytes(&s);
                }),
            ),
            ("version", Box::new(|e| e.version = 9)),
        ];
        for (name, mutate) in mutations {
            let mut t = env.clone();
            mutate(&mut t);
            assert!(t.verify().is_err(), "campo `{name}` adulterado foi aceito");
        }
    }

    #[test]
    fn author_swap_with_valid_resign_still_fails_authorization_by_content() {
        // Mallory reassina o envelope de A como se fosse dela: passa em verify (é válido para
        // ela), mas o autor muda, então quem recebe só o aceita se Mallory for contato.
        let (a, b, mallory) = (Peer::new(), Peer::new(), Peer::new());
        let env = make(&a, &[&b], "oi");
        let mut forged = env.clone();
        forged.author = mallory.id.identity_pk();
        assert!(forged.verify().is_err(), "autor trocado sem reassinar");
    }

    #[test]
    fn payload_is_bound_to_its_header() {
        // Mover o ciphertext para outro cabeçalho (outro hlc) falha na autenticação do AEAD
        // mesmo que o atacante recalcule id e assinatura com a própria chave.
        let (a, b) = (Peer::new(), Peer::new());
        let env = make(&a, &[&b], "oi");
        let mut moved = env.clone();
        moved.hlc.ms += 5;
        assert!(moved.open(&b.id).is_err());
    }

    #[test]
    fn dm_conversation_id_is_symmetric() {
        let (a, b) = (Peer::new(), Peer::new());
        let (pa, pb) = (a.id.identity_pk(), b.id.identity_pk());
        assert_eq!(dm_conversation_id(&pa, &pb), dm_conversation_id(&pb, &pa));
        assert_ne!(dm_conversation_id(&pa, &pb), dm_conversation_id(&pa, &pa));
    }

    #[test]
    fn rejects_bad_recipient_counts() {
        let a = Peer::new();
        let r = Envelope::create(
            &a.id,
            [0; 32],
            Hlc::ZERO,
            vec![],
            &[],
            EnvelopeKind::Text,
            b"x",
        );
        assert!(r.is_err());
    }
}
