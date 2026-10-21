//! Tipos de wire, framing e ALPNs. Mantido em sincronia com `docs/protocol.md`.
//!
//! Frame: `u32` big-endian com o tamanho + corpo `postcard`. O limite de tamanho é imposto
//! antes de alocar.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{identity::DeviceCert, identity::InviteTicket, Error, Result, Role};

/// ALPN do pedido de contato (único tráfego aceito de desconhecidos).
pub const ALPN_CONTACT: &[u8] = b"enxame/contact/1";
/// ALPN de sincronização; na Fase 1 só troca `Hello`. Apenas contatos mútuos.
pub const ALPN_SYNC: &[u8] = b"enxame/sync/1";
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
