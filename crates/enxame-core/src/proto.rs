//! Tipos de wire, framing e ALPNs. Mantido em sincronia com `docs/protocol.md`.
//!
//! Frame: `u32` big-endian com o tamanho + corpo `postcard`. O limite de tamanho é imposto
//! antes de alocar.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    crypto::Envelope, hlc::Hlc, identity::DeviceCert, identity::InviteTicket, Error, Result, Role,
};

/// ALPN do pedido de contato (único tráfego aceito de desconhecidos).
pub const ALPN_CONTACT: &[u8] = b"enxame/contact/1";
/// ALPN de sincronização; na Fase 1 só troca `Hello`. Apenas contatos mútuos.
pub const ALPN_SYNC: &[u8] = b"enxame/sync/1";
/// ALPN do store-and-forward (spec 9). Apenas contatos mútuos.
pub const ALPN_CARRY: &[u8] = b"enxame/carry/1";
/// ALPN da sinalização de chamadas (spec 13/Fase 6, ADR 0012). Apenas contatos mútuos.
pub const ALPN_CALL: &[u8] = b"enxame/call/1";
/// Versões de protocolo que este build entende, da mais nova para a mais antiga.
pub const SUPPORTED_VERSIONS: &[u8] = &[crate::PROTOCOL_VERSION];

/// Mensagens de `enxame/contact/1`. O iniciador envia `Request`; o receptor responde com
/// `Accept` ou `Reject` quando o usuário decidir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactMsg {
    /// Ticket de quem pede, para o receptor saber quem é e como alcançá-lo.
    Request {
        ticket: Box<InviteTicket>,
    },
    Accept,
    Reject,
}

/// Mensagens de `enxame/sync/1` depois do `Hello` (spec 7.3). A sessão é simétrica: os dois
/// lados enviam `Heads` e puxam o que lhes falta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMsg {
    /// Estado de uma conversa: pontas (envelopes sem filhos), quantidade e maior HLC.
    Heads {
        conversation_id: [u8; 32],
        heads: Vec<[u8; 32]>,
        count: u64,
        hlc_max: Hlc,
        /// BLAKE3 dos ids da conversa em ordem crescente: iguais ⇒ conjuntos iguais.
        digest: [u8; 32],
    },
    /// Pede ids com HLC `>= since_hlc` (inclusive), até `limit`.
    RangeQuery {
        conversation_id: [u8; 32],
        since_hlc: Hlc,
        limit: u32,
    },
    /// Resposta: ids em ordem; `next` é o cursor da próxima página, se houver.
    Ids {
        conversation_id: [u8; 32],
        ids: Vec<[u8; 32]>,
        next: Option<Hlc>,
    },
    /// Pede envelopes completos.
    Want {
        conversation_id: [u8; 32],
        ids: Vec<[u8; 32]>,
    },
    /// Envelopes pedidos, em um ou mais frames; `last` marca o fim da resposta a um `Want`.
    Envelopes {
        envelopes: Vec<Envelope>,
        last: bool,
    },
    /// Não tenho mais nada a pedir.
    Bye,
    /// Hashes dos meus contatos mútuos, salgados com a conversa 1:1 deste par: só quem é contato
    /// dos dois consegue reconhecê-los (contatos em comum, para escolher carriers).
    Common { hashes: Vec<[u8; 32]> },
    /// Meu papel mudou durante a sessão.
    RoleUpdate { role: Role },
}

/// Motivo de recusa de uma oferta de carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectReason {
    /// Meu papel atual não carrega para terceiros.
    NotCarrying,
    /// Estouraria a quota (por contato ou total).
    Quota,
    /// O destinatário não é meu contato mútuo (ou é o próprio ofertante).
    NotContact,
    /// Nada utilizável: assinatura inválida, autor errado, destinatário ausente, já confirmado.
    Invalid,
}

/// Blob oferecido para replicação junto com uma oferta de carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobOffer {
    /// Raiz (`HashSeq`) do arquivo cifrado.
    pub hash: [u8; 32],
    /// Tamanho do ciphertext, para o carrier decidir antes de baixar.
    pub enc_size: u64,
}

/// Mensagens de `enxame/carry/1` (spec 9). Sessão simétrica; cada `Deliver` recebe exatamente um
/// `Receipts` de volta e cada `Offer` exatamente um `Accept`/`Reject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CarryMsg {
    /// Recibos assinados (envelopes `kind = Receipt`) para o par expurgar cópias. `reply = true`
    /// quando respondem a um `Deliver`.
    Receipts {
        receipts: Vec<Envelope>,
        reply: bool,
    },
    /// Envelopes que eu carrego para **você** (o par é o destinatário).
    Deliver {
        envelopes: Vec<Envelope>,
    },
    /// Peço que você carregue estes envelopes (meus) para `for_recipient`. `blobs` são os
    /// arquivos referenciados por envelopes `FileRef` da oferta: o carrier (`Relay`/`SuperPeer`)
    /// pode replicá-los de mim para que o destinatário baixe mesmo com o meu nó offline.
    Offer {
        for_recipient: [u8; 32],
        envelopes: Vec<Envelope>,
        blobs: Vec<BlobOffer>,
    },
    Accept {
        ids: Vec<[u8; 32]>,
    },
    Reject {
        reason: RejectReason,
    },
    Bye,
}

/// Mensagens de `enxame/call/1`: um sinal de chamada (envelope `kind = CallSignal`, E2E) e a
/// confirmação de que o par o aceitou (ou a recusa).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallMsg {
    Signal {
        envelope: Box<Envelope>,
    },
    Ack,
    Reject,
    /// Recusa com o motivo (texto para quem envia mostrar ao usuário).
    Refused {
        reason: String,
    },
}

/// Primeira mensagem de `enxame/sync/1`, enviada nos dois sentidos (spec 7.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub device_cert: DeviceCert,
    pub protocol_versions: Vec<u8>,
    pub role: Role,
    /// Bitmask reservado para capacidades opcionais (zero na Fase 1).
    pub capabilities: u32,
}

impl Hello {
    /// Maior versão de protocolo comum, se houver.
    pub fn negotiate(&self, ours: &[u8]) -> Option<u8> {
        ours.iter()
            .copied()
            .filter(|v| self.protocol_versions.contains(v))
            .max()
    }
}

/// Escreve um frame.
pub async fn write_frame<W, T>(w: &mut W, msg: &T, max: usize) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let body = postcard::to_stdvec(msg)?;
    if body.len() > max {
        return Err(Error::Protocol(format!(
            "frame de {} bytes excede {max}",
            body.len()
        )));
    }
    let len = u32::try_from(body.len()).map_err(|_| Error::Protocol("frame enorme".into()))?;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&body).await?;
    Ok(())
}

/// Lê um frame, recusando tamanhos acima de `max` sem alocar.
pub async fn read_frame<R, T>(r: &mut R, max: usize) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err(Error::Protocol(format!(
            "frame de {len} bytes excede {max}"
        )));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(postcard::from_bytes(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    #[tokio::test]
    async fn hello_roundtrips_through_a_frame() {
        let id = Identity::generate();
        let hello = Hello {
            device_cert: id.device_cert(42),
            protocol_versions: vec![1],
            role: Role::Leaf,
            capabilities: 0,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &hello, 1024).await.unwrap();
        let back: Hello = read_frame(&mut buf.as_slice(), 1024).await.unwrap();
        assert_eq!(back, hello);
        assert_eq!(back.negotiate(&[1]), Some(1));
        assert_eq!(back.negotiate(&[2]), None);
    }

    #[tokio::test]
    async fn sync_msgs_roundtrip() {
        let msgs = vec![
            SyncMsg::Heads {
                conversation_id: [1; 32],
                heads: vec![[2; 32]],
                count: 3,
                hlc_max: Hlc::new(9, 1),
                digest: [8; 32],
            },
            SyncMsg::RangeQuery {
                conversation_id: [1; 32],
                since_hlc: Hlc::new(4, 0),
                limit: 100,
            },
            SyncMsg::Ids {
                conversation_id: [1; 32],
                ids: vec![[5; 32], [6; 32]],
                next: Some(Hlc::new(7, 7)),
            },
            SyncMsg::Want {
                conversation_id: [1; 32],
                ids: vec![[5; 32]],
            },
            SyncMsg::Envelopes {
                envelopes: vec![],
                last: true,
            },
            SyncMsg::Bye,
            SyncMsg::Common {
                hashes: vec![[3; 32]],
            },
            SyncMsg::RoleUpdate { role: Role::Relay },
        ];
        for msg in msgs {
            let mut buf = Vec::new();
            write_frame(&mut buf, &msg, 4096).await.unwrap();
            let back: SyncMsg = read_frame(&mut buf.as_slice(), 4096).await.unwrap();
            assert_eq!(back, msg);
        }
    }

    #[tokio::test]
    async fn carry_msgs_roundtrip() {
        let msgs = vec![
            CarryMsg::Receipts {
                receipts: vec![],
                reply: false,
            },
            CarryMsg::Deliver { envelopes: vec![] },
            CarryMsg::Offer {
                for_recipient: [4; 32],
                envelopes: vec![],
                blobs: vec![BlobOffer {
                    hash: [6; 32],
                    enc_size: 77,
                }],
            },
            CarryMsg::Accept { ids: vec![[5; 32]] },
            CarryMsg::Reject {
                reason: RejectReason::Quota,
            },
            CarryMsg::Bye,
        ];
        for msg in msgs {
            let mut buf = Vec::new();
            write_frame(&mut buf, &msg, 4096).await.unwrap();
            let back: CarryMsg = read_frame(&mut buf.as_slice(), 4096).await.unwrap();
            assert_eq!(back, msg);
        }
    }

    #[tokio::test]
    async fn call_msgs_roundtrip() {
        for msg in [
            CallMsg::Ack,
            CallMsg::Reject,
            CallMsg::Refused {
                reason: "relógio fora de hora".into(),
            },
        ] {
            let mut buf = Vec::new();
            write_frame(&mut buf, &msg, 4096).await.unwrap();
            let back: CallMsg = read_frame(&mut buf.as_slice(), 4096).await.unwrap();
            assert_eq!(back, msg);
        }
        let id = Identity::generate();
        let peer = Identity::generate();
        let env = Envelope::create(
            &id,
            [1; 32],
            Hlc::new(5, 0),
            vec![],
            &[crate::crypto::Recipient {
                identity_pk: peer.identity_pk(),
                enc_pk: peer.enc_pk(),
            }],
            crate::crypto::EnvelopeKind::CallSignal,
            b"{}",
        )
        .unwrap();
        let msg = CallMsg::Signal {
            envelope: Box::new(env),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg, 1 << 20).await.unwrap();
        let back: CallMsg = read_frame(&mut buf.as_slice(), 1 << 20).await.unwrap();
        assert_eq!(back, msg);
    }

    #[tokio::test]
    async fn contact_msgs_roundtrip() {
        for msg in [ContactMsg::Accept, ContactMsg::Reject] {
            let mut buf = Vec::new();
            write_frame(&mut buf, &msg, 1024).await.unwrap();
            let back: ContactMsg = read_frame(&mut buf.as_slice(), 1024).await.unwrap();
            assert_eq!(back, msg);
        }
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected_without_allocating() {
        let mut data = (u32::MAX).to_be_bytes().to_vec();
        data.extend_from_slice(b"x");
        let r: Result<Hello> = read_frame(&mut data.as_slice(), 1024).await;
        assert!(matches!(r, Err(Error::Protocol(_))));
    }

    #[tokio::test]
    async fn garbage_body_is_a_codec_error() {
        let mut data = 3u32.to_be_bytes().to_vec();
        data.extend_from_slice(&[0xff, 0xff, 0xff]);
        let r: Result<Hello> = read_frame(&mut data.as_slice(), 1024).await;
        assert!(r.is_err());
    }
}
