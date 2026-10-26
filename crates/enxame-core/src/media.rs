//! Ponte de mídia das chamadas (Fase 7.2, ADR 0015): um servidor TURN local em `127.0.0.1` cujo
//! "relay" leva os pacotes ao outro contato por datagramas QUIC do iroh (`enxame/media/1`).
//! Aqui ficam as peças que não dependem do nó: endereços virtuais, `Conn`, autenticação e contadores.

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use async_trait::async_trait;
use iroh::endpoint::Connection;
use rand::{distributions::DistString, rngs::OsRng};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use turn::{auth::AuthHandler, relay::RelayAddressGenerator};
use webrtc_util::Conn;

use crate::proto::{decode_media_datagram, encode_media_datagram};

/// Realm do TURN local (só aparece no cálculo da chave de autenticação).
pub(crate) const REALM: &str = "enxame";

/// Porta dos endereços virtuais. Os endereços não precisam ser roteáveis: só identificam, para o
/// libwebrtc, "o outro lado" dentro do TURN local.
const VIRTUAL_PORT: u16 = 40000;

/// Quem chamou e quem atendeu têm endereços virtuais diferentes (faixa de testes 198.18.0.0/15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeRole {
    Caller,
    Callee,
}

impl BridgeRole {
    pub(crate) fn virtual_addr(self) -> SocketAddr {
        let last = match self {
            Self::Caller => 1,
            Self::Callee => 2,
        };
        SocketAddr::from(([198, 18, 0, last], VIRTUAL_PORT))
    }

    pub(crate) fn peer(self) -> Self {
        match self {
            Self::Caller => Self::Callee,
            Self::Callee => Self::Caller,
        }
    }
}

/// Contadores da ponte (para diagnóstico e para a medição da Fase 7.1).
#[derive(Debug, Default)]
pub struct BridgeCounters {
    pub sent: AtomicU64,
    pub received: AtomicU64,
    /// Pacotes descartados porque a conexão com o contato ainda não abriu.
    pub dropped_not_ready: AtomicU64,
    /// Pacotes que não cabem em um datagrama do caminho atual.
    pub dropped_too_large: AtomicU64,
    /// Pacotes que o QUIC recusou ou descartou na fila de envio.
    pub dropped_send_error: AtomicU64,
    /// Quadros recebidos com versão desconhecida.
    pub dropped_bad_frame: AtomicU64,
}

/// Fotografia dos contadores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BridgeStats {
    pub sent: u64,
    pub received: u64,
    pub dropped_not_ready: u64,
    pub dropped_too_large: u64,
    pub dropped_send_error: u64,
    pub dropped_bad_frame: u64,
}

impl BridgeCounters {
    pub(crate) fn snapshot(&self) -> BridgeStats {
        let g = |c: &AtomicU64| c.load(Ordering::Relaxed);
        BridgeStats {
            sent: g(&self.sent),
            received: g(&self.received),
            dropped_not_ready: g(&self.dropped_not_ready),
            dropped_too_large: g(&self.dropped_too_large),
            dropped_send_error: g(&self.dropped_send_error),
            dropped_bad_frame: g(&self.dropped_bad_frame),
        }
    }
}

type Packet = (Vec<u8>, SocketAddr);

/// Estado compartilhado entre o TURN local, o laço de leitura e quem controla a ponte.
pub(crate) struct Shared {
    pub(crate) role: BridgeRole,
    /// Conexão `enxame/media/1` com o contato, quando já abriu.
    pub(crate) conn: Mutex<Option<Connection>>,
    /// Entrada dos pacotes vindos do contato; dropar encerra a leitura do relay.
    pub(crate) inbound_tx: Mutex<Option<mpsc::Sender<Packet>>>,
    pub(crate) counters: BridgeCounters,
}

impl Shared {
    pub(crate) fn new(role: BridgeRole) -> (Arc<Self>, mpsc::Receiver<Packet>) {
        let (tx, rx) = mpsc::channel(1024);
        let shared = Arc::new(Self {
            role,
            conn: Mutex::new(None),
            inbound_tx: Mutex::new(Some(tx)),
            counters: BridgeCounters::default(),
        });
        (shared, rx)
    }

    pub(crate) fn connection(&self) -> Option<Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub(crate) fn set_connection(&self, c: Option<Connection>) {
        *self.conn.lock().unwrap_or_else(|p| p.into_inner()) = c;
    }

    /// Fecha a entrada: o relay do TURN passa a ver fim de arquivo.
    pub(crate) fn close_inbound(&self) {
        self.inbound_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }

    /// Entrega ao TURN local um datagrama recebido do contato.
    pub(crate) fn deliver(&self, datagram: &[u8]) {
        let Some(payload) = decode_media_datagram(datagram) else {
            self.counters
                .dropped_bad_frame
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        let tx = self
            .inbound_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(tx) = tx {
            if tx
                .try_send((payload.to_vec(), self.role.peer().virtual_addr()))
                .is_ok()
            {
                self.counters.received.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// O "socket" de relay do TURN local: o que o libwebrtc manda ao endereço virtual do contato sai
/// como datagrama QUIC; o que o contato manda volta como pacote vindo desse endereço.
pub(crate) struct BridgeConn {
    shared: Arc<Shared>,
    rx: AsyncMutex<mpsc::Receiver<Packet>>,
}

fn io_err(kind: std::io::ErrorKind) -> webrtc_util::Error {
    webrtc_util::Error::Io(std::io::Error::from(kind).into())
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
            .ok_or_else(|| io_err(std::io::ErrorKind::BrokenPipe))?;
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        Ok((n, src))
    }

    async fn send(&self, buf: &[u8]) -> webrtc_util::Result<usize> {
        Ok(buf.len())
    }

    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> webrtc_util::Result<usize> {
        // Só o par: pares ICE do candidato relay para endereços reais não levam a lugar nenhum.
        if target != self.shared.role.peer().virtual_addr() {
            return Ok(buf.len());
        }
        let c = &self.shared.counters;
        let Some(conn) = self.shared.connection() else {
            c.dropped_not_ready.fetch_add(1, Ordering::Relaxed);
            return Ok(buf.len());
        };
        let frame = encode_media_datagram(buf);
        if conn.max_datagram_size().is_none_or(|max| frame.len() > max) {
            c.dropped_too_large.fetch_add(1, Ordering::Relaxed);
            return Ok(buf.len());
        }
        match conn.send_datagram(frame) {
            Ok(()) => c.sent.fetch_add(1, Ordering::Relaxed),
            Err(_) => c.dropped_send_error.fetch_add(1, Ordering::Relaxed),
        };
        Ok(buf.len())
    }

    fn local_addr(&self) -> webrtc_util::Result<SocketAddr> {
        Ok(self.shared.role.virtual_addr())
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

/// Entrega ao TURN local o relay da primeira alocação. O libwebrtc pede uma alocação por interface
/// de rede (12 numa máquina com Docker); todas anunciariam o mesmo endereço virtual, então só a
/// primeira vale e as demais recebem erro (verificado no spike do 7.2: continua conectando).
pub(crate) struct BridgeGen {
    pub(crate) shared: Arc<Shared>,
    pub(crate) rx: Mutex<Option<mpsc::Receiver<Packet>>>,
    pub(crate) taken: AtomicBool,
}

#[async_trait]
impl RelayAddressGenerator for BridgeGen {
    fn validate(&self) -> Result<(), turn::Error> {
        Ok(())
    }

    async fn allocate_conn(
        &self,
        use_ipv4: bool,
        _requested_port: u16,
    ) -> Result<(Arc<dyn Conn + Send + Sync>, SocketAddr), turn::Error> {
        if !use_ipv4 {
            return Err(turn::Error::ErrListeningAddressInvalid);
        }
        if self.taken.swap(true, Ordering::SeqCst) {
            return Err(turn::Error::ErrMaxRetriesExceeded);
        }
        let rx = self
            .rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .ok_or(turn::Error::ErrMaxRetriesExceeded)?;
        let conn = BridgeConn {
            shared: self.shared.clone(),
            rx: AsyncMutex::new(rx),
        };
        Ok((Arc::new(conn), self.shared.role.virtual_addr()))
    }
}

/// Um usuário e senha aleatórios por chamada.
pub(crate) struct OneUser {
    pub(crate) user: String,
    pub(crate) key: Vec<u8>,
}

impl AuthHandler for OneUser {
    fn auth_handle(
        &self,
        username: &str,
        _realm: &str,
        _src: SocketAddr,
    ) -> Result<Vec<u8>, turn::Error> {
        if username == self.user {
            Ok(self.key.clone())
        } else {
            Err(turn::Error::ErrFakeErr)
        }
    }
}

/// Texto aleatório alfanumérico do gerador do sistema.
pub(crate) fn random_token(len: usize) -> String {
    rand::distributions::Alphanumeric.sample_string(&mut OsRng, len)
}
