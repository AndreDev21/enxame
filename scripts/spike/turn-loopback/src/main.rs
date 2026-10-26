//! Dois servidores TURN em 127.0.0.1 (lados A e B) cujo relay entrega os pacotes de um ao outro em
//! memória, nos endereços virtuais 198.18.0.1:40000 (A) e 198.18.0.2:40000 (B). Imprime uma linha
//! JSON com porta, usuário e senha de cada lado e roda até o stdin fechar.
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rand::distr::{Alphanumeric, SampleString};
use tokio::io::AsyncReadExt;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use turn::auth::{generate_auth_key, AuthHandler};
use turn::relay::RelayAddressGenerator;
use turn::server::config::{ConnConfig, ServerConfig};
use turn::server::Server;
use turn::Error as TurnError;
use webrtc_util::Conn;

const REALM: &str = "enxame";

type Packet = (Vec<u8>, SocketAddr);

fn virtual_addr(side: usize) -> SocketAddr {
    format!("198.18.0.{}:40000", side + 1).parse().expect("endereço fixo")
}

/// Ponto de encontro: a entrada de cada lado recebe o que o outro envia.
struct Hub {
    inbox: [Mutex<Option<mpsc::Sender<Packet>>>; 2],
}

struct BridgeConn {
    me: usize,
    hub: Arc<Hub>,
    rx: AsyncMutex<mpsc::Receiver<Packet>>,
}

#[async_trait]
impl Conn for BridgeConn {
    async fn connect(&self, _addr: SocketAddr) -> webrtc_util::Result<()> {
        Ok(())
    }
    async fn recv(&self, buf: &mut [u8]) -> webrtc_util::Result<usize> {
        Ok(self.recv_from(buf).await?.0)
    }
    async fn recv_from(&self, buf: &mut [u8]) -> webrtc_util::Result<(usize, SocketAddr)> {
        let (data, src) = self
            .rx
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        Ok((n, src))
    }
    async fn send(&self, buf: &[u8]) -> webrtc_util::Result<usize> {
        Ok(buf.len())
    }
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> webrtc_util::Result<usize> {
        let other = 1 - self.me;
        // Só o par: qualquer outro destino (candidatos reais) é descartado.
        if target == virtual_addr(other) {
            let tx = self.hub.inbox[other].lock().expect("hub").clone();
            if let Some(tx) = tx {
                let _ = tx.try_send((buf.to_vec(), virtual_addr(self.me)));
            }
        }
        Ok(buf.len())
    }
    fn local_addr(&self) -> webrtc_util::Result<SocketAddr> {
        Ok(virtual_addr(self.me))
    }
    fn remote_addr(&self) -> Option<SocketAddr> {
        None
    }
    async fn close(&self) -> webrtc_util::Result<()> {
        Ok(())
    }
    fn as_any(&self) -> &(dyn std::any::Any + Send + Sync) {
        self
    }
}

struct BridgeGen {
    me: usize,
    hub: Arc<Hub>,
    /// O libwebrtc pede uma alocação por interface de rede; só a primeira vale (uma por chamada).
    taken: AtomicBool,
}

#[async_trait]
impl RelayAddressGenerator for BridgeGen {
    fn validate(&self) -> Result<(), TurnError> {
        Ok(())
    }
    async fn allocate_conn(
        &self,
        use_ipv4: bool,
        _requested_port: u16,
    ) -> Result<(Arc<dyn Conn + Send + Sync>, SocketAddr), TurnError> {
        if !use_ipv4 {
            return Err(TurnError::ErrListeningAddressInvalid);
        }
        if self.taken.swap(true, Ordering::SeqCst) {
            eprintln!("[spike] lado {}: alocação extra recusada", self.me);
            return Err(TurnError::ErrMaxRetriesExceeded);
        }
        let (tx, rx) = mpsc::channel(1024);
        *self.hub.inbox[self.me].lock().expect("hub") = Some(tx);
        eprintln!("[spike] lado {} alocou o relay {}", self.me, virtual_addr(self.me));
        let conn = BridgeConn { me: self.me, hub: self.hub.clone(), rx: AsyncMutex::new(rx) };
        Ok((Arc::new(conn), virtual_addr(self.me)))
    }
}

struct OneUser {
    user: String,
    key: Vec<u8>,
}

impl AuthHandler for OneUser {
    fn auth_handle(&self, username: &str, _realm: &str, _src: SocketAddr) -> Result<Vec<u8>, TurnError> {
        if username == self.user {
            Ok(self.key.clone())
        } else {
            Err(TurnError::ErrFakeErr)
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hub = Arc::new(Hub { inbox: [Mutex::new(None), Mutex::new(None)] });
    let mut servers = Vec::new();
    let mut json = Vec::new();
    for side in 0..2 {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
        let port = sock.local_addr()?.port();
        let user = Alphanumeric.sample_string(&mut rand::rng(), 16);
        let pass = Alphanumeric.sample_string(&mut rand::rng(), 24);
        let server = Server::new(ServerConfig {
            conn_configs: vec![ConnConfig {
                conn: sock,
                relay_addr_generator: Box::new(BridgeGen { me: side, hub: hub.clone(), taken: AtomicBool::new(false) }),
            }],
            realm: REALM.to_owned(),
            auth_handler: Arc::new(OneUser { key: generate_auth_key(&user, REALM, &pass), user: user.clone() }),
            channel_bind_timeout: std::time::Duration::from_secs(600),
            alloc_close_notify: None,
        })
        .await?;
        servers.push(server);
        json.push(format!(r#""{}":{{"port":{port},"user":"{user}","pass":"{pass}"}}"#, if side == 0 { "a" } else { "b" }));
    }
    println!("{{{}}}", json.join(","));
    // Roda até o processo pai fechar o stdin.
    let mut sink = Vec::new();
    let _ = tokio::io::stdin().read_to_end(&mut sink).await;
    for s in servers {
        let _ = s.close().await;
    }
    Ok(())
}
