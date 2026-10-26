//! Ponte de mídia no nó (Fase 7.2, ADR 0015): servidor TURN local em loopback + conexão
//! `enxame/media/1` com o contato. O app usa [`MediaBridge::url`], `username` e `credential` como
//! `iceServers` do WebRTC.

use std::{
    net::SocketAddr,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};

use iroh::endpoint::Connection;
use tokio::{net::UdpSocket, sync::oneshot, task::JoinHandle};
use turn::{
    auth::generate_auth_key,
    server::{
        config::{ConnConfig, ServerConfig},
        Server,
    },
};

use super::{Enxame, Inner, CLOSE_CODE_NOT_CONTACT};
use crate::{
    media::{random_token, BridgeGen, BridgeRole, BridgeStats, OneUser, Shared, REALM},
    proto::ALPN_MEDIA,
    Error, Result,
};

/// Quanto o chamador insiste em discar o contato antes de desistir da ponte.
const DIAL_BUDGET: Duration = Duration::from_secs(20);
/// Quanto o atendente espera o chamador discar.
const ACCEPT_BUDGET: Duration = Duration::from_secs(120);

/// Resultado de [`Enxame::media_bridge_start`]. Fechar (ou largar) a ponte encerra o TURN local e
/// a conexão com o contato.
pub struct MediaBridge {
    /// `turn:127.0.0.1:PORTA?transport=udp`.
    pub url: String,
    pub username: String,
    pub credential: String,
    shared: Arc<Shared>,
    server: Option<Server>,
    link: JoinHandle<()>,
    inner: Arc<Inner>,
    peer_key: [u8; 32],
}

impl MediaBridge {
    /// Contadores de pacotes enviados, recebidos e descartados.
    pub fn stats(&self) -> BridgeStats {
        self.shared.counters.snapshot()
    }

    /// A conexão `enxame/media/1` com o contato já abriu.
    pub fn is_linked(&self) -> bool {
        self.shared.connection().is_some()
    }

    /// Maior pacote UDP (já com o byte de versão) que cabe agora num datagrama QUIC; muda com a
    /// descoberta de MTU do caminho.
    pub fn max_datagram_size(&self) -> Option<usize> {
        self.shared.connection()?.max_datagram_size()
    }

    /// Caminho que o iroh usa para esta conexão: `true` = direto, `false` = por relay. `None` se a
    /// conexão ainda não abriu.
    pub fn is_direct(&self) -> Option<bool> {
        let conn = self.shared.connection()?;
        let direct = conn.paths().iter().any(|p| p.is_selected() && p.is_ip());
        Some(direct)
    }

    pub async fn close(mut self) {
        self.shutdown();
        if let Some(server) = self.server.take() {
            let _ = server.close().await;
        }
    }

    fn shutdown(&mut self) {
        self.link.abort();
        self.shared.close_inbound();
        if let Some(c) = self.shared.connection() {
            c.close(0u32.into(), b"media done");
        }
        self.shared.set_connection(None);
        self.inner
            .media_pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.peer_key);
    }
}

impl Drop for MediaBridge {
    fn drop(&mut self) {
        self.shutdown();
        if let (Some(server), Ok(rt)) = (self.server.take(), tokio::runtime::Handle::try_current())
        {
            rt.spawn(async move {
                let _ = server.close().await;
            });
        }
    }
}

impl Enxame {
    /// Sobe o TURN local para uma chamada com `contact` (contato mútuo) e começa a ligar a ponte.
    /// O chamador disca o contato; o atendente espera ser discado, então deve chamar isto **antes**
    /// de responder `accept`. A função volta já com as credenciais: a conexão com o contato abre
    /// em segundo plano (pacotes enviados antes disso são descartados e contados).
    pub async fn media_bridge_start(&self, contact: &str, role: BridgeRole) -> Result<MediaBridge> {
        let inner = self.inner.clone();
        let c = inner.book().resolve(contact)?.clone();
        if !c.is_mutual() {
            return Err(Error::Contact(
                "o contato ainda não aceitou (não é mútuo)".into(),
            ));
        }
        let peer_key = *c.node_addr.id.as_bytes();

        let (shared, rx) = Shared::new(role);
        let sock = Arc::new(
            UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                .await
                .map_err(|e| Error::Net(format!("TURN local: {e}")))?,
        );
        let port = sock
            .local_addr()
            .map_err(|e| Error::Net(format!("TURN local: {e}")))?
            .port();
        let (username, credential) = (random_token(16), random_token(24));
        let server = Server::new(ServerConfig {
            conn_configs: vec![ConnConfig {
                conn: sock,
                relay_addr_generator: Box::new(BridgeGen {
                    shared: shared.clone(),
                    rx: Mutex::new(Some(rx)),
                    taken: AtomicBool::new(false),
                }),
            }],
            realm: REALM.to_owned(),
            auth_handler: Arc::new(OneUser {
                key: generate_auth_key(&username, REALM, &credential),
                user: username.clone(),
            }),
            channel_bind_timeout: Duration::from_secs(600),
            alloc_close_notify: None,
        })
        .await
        .map_err(|e| Error::Net(format!("TURN local: {e}")))?;

        // O atendente se registra antes de devolver o controle: o chamador pode discar de imediato.
        let accepted = match role {
            BridgeRole::Callee => {
                let (tx, rx) = oneshot::channel();
                inner
                    .media_pending
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(peer_key, tx);
                Some(rx)
            }
            BridgeRole::Caller => None,
        };
        let link = tokio::spawn({
            let mut accepted = accepted;
            let (inner, shared, addr) = (inner.clone(), shared.clone(), c.node_addr.clone());
            async move {
                let started = tokio::time::Instant::now();
                loop {
                    let conn = match accepted.take() {
                        Some(rx) => tokio::time::timeout(ACCEPT_BUDGET, rx)
                            .await
                            .ok()
                            .and_then(|r| r.ok()),
                        None => dial(&inner, addr.clone()).await,
                    };
                    let Some(conn) = conn else {
                        tracing::debug!("ponte de mídia: sem conexão com o contato");
                        return;
                    };
                    let opened = tokio::time::Instant::now();
                    shared.set_connection(Some(conn.clone()));
                    while let Ok(d) = conn.read_datagram().await {
                        shared.deliver(&d);
                    }
                    shared.set_connection(None);
                    // Se o contato fechou na hora (ainda não tinha pedido a ponte) o chamador
                    // tenta de novo dentro do orçamento; uma conexão que viveu é um fim normal.
                    if role == BridgeRole::Callee
                        || opened.elapsed() > Duration::from_secs(3)
                        || started.elapsed() > DIAL_BUDGET
                    {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        });
        Ok(MediaBridge {
            url: format!("turn:127.0.0.1:{port}?transport=udp"),
            username,
            credential,
            shared,
            server: Some(server),
            link,
            inner,
            peer_key,
        })
    }
}

async fn dial(inner: &Inner, addr: iroh::EndpointAddr) -> Option<Connection> {
    let deadline = tokio::time::Instant::now() + DIAL_BUDGET;
    let per_try = Duration::from_secs(inner.cfg.dial_timeout_secs);
    while tokio::time::Instant::now() < deadline {
        let attempt = inner.endpoint.connect(addr.clone(), ALPN_MEDIA);
        if let Ok(Ok(conn)) = tokio::time::timeout(per_try, attempt).await {
            return Some(conn);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    None
}

/// Lado que recebe: só contatos mútuos, e só se o app já pediu uma ponte para eles.
pub(super) async fn handle_media(inner: &Inner, conn: &Connection) -> Result<()> {
    let remote = conn.remote_id();
    if inner.book().mutual_by_endpoint(&remote).is_none() {
        conn.close(CLOSE_CODE_NOT_CONTACT.into(), b"not a mutual contact");
        return Ok(());
    }
    let waiting = inner
        .media_pending
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(remote.as_bytes());
    match waiting {
        Some(tx) => {
            // Quem recebe passa a ser o dono da conexão; se já desistiu, ela se fecha ao ser largada.
            let _ = tx.send(conn.clone());
        }
        None => conn.close(404u32.into(), b"no media bridge requested"),
    }
    Ok(())
}
