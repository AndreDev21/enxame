//! Identidade: chave Ed25519 do usuário, chave X25519 de cifragem, chave de dispositivo
//! (a `SecretKey` do iroh), `DeviceCert` e `InviteTicket` (spec 6).

use std::{fmt, str::FromStr};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use iroh::{EndpointAddr, EndpointId, SecretKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::{keystore::KeyStore, Error, Result};

const DOMAIN_CERT: &[u8] = b"enxame/devicecert/1\0";
const DOMAIN_INVITE: &[u8] = b"enxame/invite/1\0";
const TICKET_PREFIX: &str = "enxame1";
/// Versão atual do formato de `InviteTicket`.
pub const TICKET_VERSION: u8 = 1;
const MAX_NAME_LEN: usize = 64;
const MAX_TICKET_TTL_SECS: u64 = 30 * 24 * 3600;

/// Segundos desde a época Unix.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Prefixo hexadecimal curto da chave de identidade, para exibição e referência na CLI.
pub fn fingerprint(identity_pk: &[u8; 32]) -> String {
    data_encoding::HEXLOWER.encode(&identity_pk[..8])
}

/// Certificado que liga a identidade a um dispositivo (`NodeId` do iroh).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCert {
    pub identity_pk: [u8; 32],
    pub node_id: [u8; 32],
    pub created_at: u64,
    pub sig: Signature,
}

impl DeviceCert {
    fn signing_bytes(identity_pk: &[u8; 32], node_id: &[u8; 32], created_at: u64) -> Vec<u8> {
        let mut v = Vec::with_capacity(DOMAIN_CERT.len() + 72);
        v.extend_from_slice(DOMAIN_CERT);
        v.extend_from_slice(identity_pk);
        v.extend_from_slice(node_id);
        v.extend_from_slice(&created_at.to_le_bytes());
        v
    }

    /// Verifica a assinatura da identidade sobre (identidade, dispositivo, data).
    pub fn verify(&self) -> Result<()> {
        let pk = VerifyingKey::from_bytes(&self.identity_pk)
            .map_err(|e| Error::InvalidTicket(format!("identity_pk: {e}")))?;
        let msg = Self::signing_bytes(&self.identity_pk, &self.node_id, self.created_at);
        pk.verify(&msg, &self.sig)
            .map_err(|_| Error::InvalidTicket("assinatura do DeviceCert inválida".into()))
    }

    /// `EndpointId` do iroh a que o certificado se refere.
    pub fn endpoint_id(&self) -> Result<EndpointId> {
        EndpointId::from_bytes(&self.node_id)
            .map_err(|e| Error::InvalidTicket(format!("node_id: {e}")))
    }
}

/// Convite para virar contato (spec 6.2). Assinado pela chave de identidade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteTicket {
    pub version: u8,
    pub identity_pk: [u8; 32],
    pub enc_pk: [u8; 32],
    pub device_cert: DeviceCert,
    /// Endereço do dispositivo (`NodeAddr` na spec; `EndpointAddr` no iroh 1.x).
    pub node_addr: EndpointAddr,
    pub display_name: String,
    pub expires_at: u64,
    pub sig: Signature,
}

impl InviteTicket {
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        let mut unsigned = self.clone();
        unsigned.sig = Signature::from_bytes(&[0u8; 64]);
        let mut v = DOMAIN_INVITE.to_vec();
        v.extend(postcard::to_stdvec(&unsigned)?);
        Ok(v)
    }

    /// Valida versão, validade, assinatura do ticket e do `DeviceCert`, e a consistência entre
    /// o certificado e o endereço. Não confia em nada antes de passar por aqui.
    pub fn verify(&self, now: u64) -> Result<()> {
        if self.version != TICKET_VERSION {
            return Err(Error::InvalidTicket(format!(
                "versão {} não suportada",
                self.version
            )));
        }
        if self.expires_at <= now {
            return Err(Error::InvalidTicket("ticket expirado".into()));
        }
        if self.expires_at > now.saturating_add(MAX_TICKET_TTL_SECS) {
            return Err(Error::InvalidTicket("validade excessiva".into()));
        }
        if self.display_name.chars().count() > MAX_NAME_LEN {
            return Err(Error::InvalidTicket("nome longo demais".into()));
        }
        let pk = VerifyingKey::from_bytes(&self.identity_pk)
            .map_err(|e| Error::InvalidTicket(format!("identity_pk: {e}")))?;
        pk.verify(&self.signing_bytes()?, &self.sig)
            .map_err(|_| Error::InvalidTicket("assinatura inválida".into()))?;
        self.device_cert.verify()?;
        if self.device_cert.identity_pk != self.identity_pk {
            return Err(Error::InvalidTicket(
                "DeviceCert de outra identidade".into(),
            ));
        }
        if self.device_cert.endpoint_id()? != self.node_addr.id {
            return Err(Error::InvalidTicket(
                "DeviceCert não corresponde ao endereço".into(),
            ));
        }
        Ok(())
    }

    /// Codifica como `enxame1<base32>` (também é o payload do QR).
    pub fn encode(&self) -> Result<String> {
        let bytes = postcard::to_stdvec(self)?;
        Ok(format!(
            "{TICKET_PREFIX}{}",
            data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
        ))
    }

    /// QR do ticket em blocos Unicode para terminal.
    pub fn qr_text(&self) -> Result<String> {
        let code = qrcode::QrCode::new(self.encode()?.as_bytes())
            .map_err(|e| Error::Codec(format!("QR: {e}")))?;
        Ok(code
            .render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build())
    }
}

impl fmt::Display for InviteTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.encode() {
            Ok(s) => f.write_str(&s),
            Err(_) => f.write_str("<ticket inválido>"),
        }
    }
}

impl FromStr for InviteTicket {
    type Err = Error;

    /// Decodifica sem validar assinatura; chame [`InviteTicket::verify`] antes de usar.
    fn from_str(s: &str) -> Result<Self> {
        let body = s
            .trim()
            .strip_prefix(TICKET_PREFIX)
            .ok_or_else(|| Error::InvalidTicket(format!("prefixo `{TICKET_PREFIX}` ausente")))?;
        let bytes = data_encoding::BASE32_NOPAD
            .decode(body.to_uppercase().as_bytes())
            .map_err(|e| Error::InvalidTicket(format!("base32: {e}")))?;
        postcard::from_bytes(&bytes).map_err(|e| Error::InvalidTicket(format!("formato: {e}")))
    }
}

/// Material de chaves persistido (um blob no keystore).
#[derive(Serialize, Deserialize)]
struct KeyMaterial {
    version: u8,
    identity: [u8; 32],
    enc: [u8; 32],
    device: [u8; 32],
}

/// Identidade completa do usuário neste dispositivo.
pub struct Identity {
    signing: SigningKey,
    enc_secret: x25519_dalek::StaticSecret,
    device: SecretKey,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &fingerprint(&self.identity_pk()))
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Gera chaves novas.
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
            enc_secret: x25519_dalek::StaticSecret::random_from_rng(OsRng),
            device: SecretKey::generate(),
        }
    }

    /// Carrega do keystore ou gera e persiste. O booleano indica se foi criada agora.
    pub fn load_or_create(store: &dyn KeyStore) -> Result<(Self, bool)> {
        if let Some(bytes) = store.load()? {
            let m: KeyMaterial = postcard::from_bytes(&bytes)
                .map_err(|e| Error::KeyStore(format!("material de chaves corrompido: {e}")))?;
            if m.version != 1 {
                return Err(Error::KeyStore(format!(
                    "versão {} do material de chaves não suportada",
                    m.version
                )));
            }
            return Ok((
                Self {
                    signing: SigningKey::from_bytes(&m.identity),
                    enc_secret: x25519_dalek::StaticSecret::from(m.enc),
                    device: SecretKey::from_bytes(&m.device),
                },
                false,
            ));
        }
        let id = Self::generate();
        let m = KeyMaterial {
            version: 1,
            identity: id.signing.to_bytes(),
            enc: id.enc_secret.to_bytes(),
            device: id.device.to_bytes(),
        };
        store.save(&postcard::to_stdvec(&m)?)?;
        Ok((id, true))
    }

    /// Chave pública de identidade (Ed25519).
    pub fn identity_pk(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// Chave pública de cifragem (X25519).
    pub fn enc_pk(&self) -> [u8; 32] {
        x25519_dalek::PublicKey::from(&self.enc_secret).to_bytes()
    }

    /// Chave secreta do dispositivo, usada pelo endpoint iroh.
    pub fn device_secret(&self) -> &SecretKey {
        &self.device
    }

    /// `EndpointId` (NodeId) deste dispositivo.
    pub fn endpoint_id(&self) -> EndpointId {
        self.device.public()
    }

    /// Emite o certificado ligando esta identidade ao dispositivo.
    pub fn device_cert(&self, created_at: u64) -> DeviceCert {
        let identity_pk = self.identity_pk();
        let node_id = *self.endpoint_id().as_bytes();
        let sig = self.signing.sign(&DeviceCert::signing_bytes(
            &identity_pk,
            &node_id,
            created_at,
        ));
        DeviceCert {
            identity_pk,
            node_id,
            created_at,
            sig,
        }
    }

    /// Cria um ticket de convite assinado.
    pub fn invite(
        &self,
        node_addr: EndpointAddr,
        display_name: &str,
        now: u64,
        ttl_secs: u64,
    ) -> Result<InviteTicket> {
        let mut ticket = InviteTicket {
            version: TICKET_VERSION,
            identity_pk: self.identity_pk(),
            enc_pk: self.enc_pk(),
            device_cert: self.device_cert(now),
            node_addr,
            display_name: display_name.chars().take(MAX_NAME_LEN).collect(),
            expires_at: now + ttl_secs.min(MAX_TICKET_TTL_SECS),
            sig: Signature::from_bytes(&[0u8; 64]),
        };
        ticket.sig = self.signing.sign(&ticket.signing_bytes()?);
        Ok(ticket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::FileKeyStore;

    fn ticket(id: &Identity, now: u64) -> InviteTicket {
        let addr =
            EndpointAddr::new(id.endpoint_id()).with_ip_addr("192.0.2.1:4000".parse().unwrap());
        id.invite(addr, "alice", now, 3600).unwrap()
    }

    #[test]
    fn keys_persist_across_loads() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileKeyStore::new(dir.path().join("k"));
        let (a, created) = Identity::load_or_create(&store).unwrap();
        assert!(created);
        let (b, created) = Identity::load_or_create(&store).unwrap();
        assert!(!created);
        assert_eq!(a.identity_pk(), b.identity_pk());
        assert_eq!(a.enc_pk(), b.enc_pk());
        assert_eq!(a.endpoint_id(), b.endpoint_id());
    }

    #[test]
    fn corrupted_key_material_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileKeyStore::new(dir.path().join("k"));
        store.save(b"\x07lixo").unwrap();
        assert!(Identity::load_or_create(&store).is_err());
    }

    #[test]
    fn device_cert_verifies_and_rejects_tampering() {
        let id = Identity::generate();
        let mut cert = id.device_cert(1000);
        cert.verify().unwrap();
        cert.created_at += 1;
        assert!(cert.verify().is_err());
    }

    #[test]
    fn ticket_roundtrip_through_text() {
        let id = Identity::generate();
        let t = ticket(&id, 1000);
        let s = t.to_string();
        assert!(s.starts_with("enxame1"));
        let back: InviteTicket = s.parse().unwrap();
        assert_eq!(back, t);
        back.verify(1001).unwrap();
        assert!(t.qr_text().unwrap().lines().count() > 5);
    }

    #[test]
    fn ticket_rejects_expiry_tampering_and_bad_prefix() {
        let id = Identity::generate();
        let t = ticket(&id, 1000);
        assert!(t.verify(1000 + 3600).is_err(), "expirado");

        let mut forged = t.clone();
        forged.display_name = "mallory".into();
        assert!(forged.verify(1001).is_err(), "nome adulterado");

        let mut forged = t.clone();
        forged.enc_pk[0] ^= 1;
        assert!(forged.verify(1001).is_err(), "enc_pk adulterada");

        let mut wrong_version = t.clone();
        wrong_version.version = 9;
        assert!(wrong_version.verify(1001).is_err());

        assert!("xyz".parse::<InviteTicket>().is_err());
        assert!("enxame1!!!".parse::<InviteTicket>().is_err());
        assert!("enxame1mfrgg".parse::<InviteTicket>().is_err());
    }

    #[test]
    fn ticket_with_cert_of_other_device_is_rejected() {
        let a = Identity::generate();
        let b = Identity::generate();
        // Endereço de B com certificado de A, assinado por A.
        let addr = EndpointAddr::new(b.endpoint_id());
        let t = a.invite(addr, "a", 1000, 3600).unwrap();
        assert!(t.verify(1001).is_err());
    }
}
