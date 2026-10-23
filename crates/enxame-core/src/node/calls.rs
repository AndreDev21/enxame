//! Sinalização de chamadas no nó: enviar e receber sinais E2E (ADR 0012).

use std::time::Duration;

use iroh::endpoint::Connection;

use super::{Enxame, Inner, CLOSE_CODE_NOT_CONTACT};
use crate::{
    calls::{make_signal, open_signal},
    hlc::Hlc,
    identity::unix_now_ms,
    proto::{read_frame, write_frame, CallMsg, ALPN_CALL},
    Error, Event, Result,
};

const ACK_TIMEOUT: Duration = Duration::from_secs(5);

impl Enxame {
    /// Envia um sinal de chamada (SDP, candidato ICE, desligar...) a um contato mútuo. É
    /// tempo real: se o contato estiver inacessível dá erro, sem fila nem carrier.
    pub async fn send_call_signal(&self, contact: &str, payload: &[u8]) -> Result<()> {
        let inner = &*self.inner;
        let c = inner.book().resolve(contact)?.clone();
        if !c.is_mutual() {
            return Err(Error::Contact(
                "o contato ainda não aceitou (não é mútuo)".into(),
            ));
        }
        // Carimbo de parede, não o HLC: o HLC pode andar adiantado (mensagens de pares com o relógio
        // à frente) e o sinal seria recusado por "fora do prazo".
        let env = make_signal(&inner.identity, &c, Hlc::new(unix_now_ms(), 0), payload)?;
        let dial = inner.endpoint.connect(c.node_addr.clone(), ALPN_CALL);
        let conn = tokio::time::timeout(Duration::from_secs(inner.cfg.dial_timeout_secs), dial)
            .await
            .map_err(|_| Error::Timeout("conectar ao contato (chamada)"))?
            .map_err(|e| Error::Net(format!("conectar: {e}")))?;
        let result = async {
            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|e| Error::Net(format!("abrir stream: {e}")))?;
            let max = inner.cfg.max_frame_bytes;
            write_frame(
                &mut send,
                &CallMsg::Signal {
                    envelope: Box::new(env),
                },
                max,
            )
            .await?;
            let _ = send.finish();
            match tokio::time::timeout(ACK_TIMEOUT, read_frame::<_, CallMsg>(&mut recv, max))
                .await
                .map_err(|_| Error::Timeout("confirmação do sinal"))??
            {
                CallMsg::Ack => Ok(()),
                CallMsg::Refused { reason } => Err(Error::Protocol(format!(
                    "o contato recusou o sinal ({reason})"
                ))),
                _ => Err(Error::Protocol("o contato recusou o sinal".into())),
            }
        }
        .await;
        conn.close(0u32.into(), b"signal done");
        result
    }
}

/// Lado que recebe: só contatos mútuos; valida, abre e anuncia ao app.
pub(super) async fn handle_call(inner: &Inner, conn: &Connection) -> Result<()> {
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
    let max = inner.cfg.max_frame_bytes;
    let msg: CallMsg = tokio::time::timeout(ACK_TIMEOUT, read_frame(&mut recv, max))
        .await
        .map_err(|_| Error::Timeout("sinal de chamada"))??;
    let reply = match msg {
        CallMsg::Signal { envelope } => {
            let opened = {
                let mut guard = inner.call_guard.lock().unwrap_or_else(|p| p.into_inner());
                open_signal(
                    &inner.identity,
                    &contact,
                    &envelope,
                    unix_now_ms(),
                    &mut guard,
                )
            };
            match opened {
                Ok(payload) => {
                    inner.emit(Event::CallSignal {
                        from_name: contact.display_name.clone(),
                        from_fingerprint: contact.fingerprint(),
                        payload,
                    });
                    CallMsg::Ack
                }
                Err(e) => {
                    tracing::debug!(peer = %contact.fingerprint(), "sinal de chamada recusado: {e}");
                    CallMsg::Refused {
                        reason: e.to_string(),
                    }
                }
            }
        }
        _ => CallMsg::Reject,
    };
    write_frame(&mut send, &reply, max).await?;
    let _ = send.finish();
    let _ = tokio::time::timeout(ACK_TIMEOUT, conn.closed()).await;
    Ok(())
}
