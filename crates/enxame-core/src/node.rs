//! `Enxame`: ponto de entrada da API do core (spec 3.1).

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

use iroh::{endpoint::Connection, Endpoint, EndpointAddr, EndpointId};
use tokio::{
    sync::{broadcast, oneshot},
    task::JoinHandle,
};

use crate::{
    config::Config,
    contacts::{Contact, ContactBook, ContactStatus},
    identity::{fingerprint, unix_now, Identity, InviteTicket},
    keystore::KeyStore,
    net::{self, NatReport, PathMetrics, PathMonitor},
    proto::{self, ContactMsg, Hello, ALPN_CONTACT, ALPN_SYNC, SUPPORTED_VERSIONS},
    Error, Event, Result, Role,
};

const MAX_PENDING_REQUESTS: usize = 16;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const ONLINE_TIMEOUT: Duration = Duration::from_secs(5);
const NAT_TIMEOUT: Duration = Duration::from_secs(10);
/// Quanto `diag` espera por um caminho direto antes de reportar o que tem.
pub const DEFAULT_DIRECT_WAIT: Duration = Duration::from_secs(10);
const CLOSE_CODE_NOT_CONTACT: u32 = 403;

/// Pedido de contato aguardando decisão do usuário.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRequest {
    pub id: u64,
    pub display_name: String,
    pub fingerprint: String,
}

/// Resultado de [`Enxame::add_contact`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// O outro lado aceitou; ambos têm `mutual = true`.
    Accepted(Contact),
    /// O outro lado recusou.
    Rejected,
    /// Já era contato mútuo.
    AlreadyContact(Contact),
}

/// Diagnóstico de rede de um contato (spec 11).
#[derive(Debug, Clone)]
pub struct PeerDiag {
    pub name: String,
    pub fingerprint: String,
    pub error: Option<String>,
    pub connect_time: Option<Duration>,
    pub peer_role: Option<Role>,
    pub protocol_version: Option<u8>,
    pub metrics: Option<PathMetrics>,
}

/// Diagnóstico local + por contato.
#[derive(Debug, Clone)]
pub struct DiagReport {
    pub endpoint_id: EndpointId,
    pub addr: EndpointAddr,
    pub nat: NatReport,
    /// Papel atual. Fixo em `Leaf` até a Fase 3 (conservador).
    pub role: Role,
    pub keystore: &'static str,
    pub peers: Vec<PeerDiag>,
}

struct PendingEntry {
    info: PendingRequest,
    decide: oneshot::Sender<bool>,
}

struct Inner {
    cfg: Config,
    identity: Identity,
    keystore_name: &'static str,
    endpoint: Endpoint,
    contacts: Mutex<ContactBook>,
    events: broadcast::Sender<Event>,
    pending: Mutex<HashMap<u64, PendingEntry>>,
    next_request: AtomicU64,
}

impl Inner {
    fn book(&self) -> MutexGuard<'_, ContactBook> {
        self.contacts.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn pending(&self) -> MutexGuard<'_, HashMap<u64, PendingEntry>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, ev: Event) {
        // Sem assinantes não é erro: o core não presume UI ativa.
        let _ = self.events.send(ev);
    }

    fn our_hello(&self) -> Hello {
        Hello {
            device_cert: self.identity.device_cert(unix_now()),
            protocol_versions: SUPPORTED_VERSIONS.to_vec(),
            role: Role::Leaf,
            capabilities: 0,
        }
    }
}

/// Núcleo do Enxame. Barato de clonar.
#[derive(Clone)]
pub struct Enxame {
    inner: Arc<Inner>,
    accept_task: Arc<JoinHandle<()>>,
}

impl std::fmt::Debug for Enxame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Enxame")
            .field(
                "endpoint_id",
                &self.inner.endpoint.id().fmt_short().to_string(),
            )
            .finish_non_exhaustive()
    }
}

impl Enxame {
    /// Versão do crate, para `--version` e diagnóstico.
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Carrega (ou cria) a identidade, abre a agenda e sobe o endpoint.
    pub async fn start(cfg: Config, keystore: &dyn KeyStore) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let (identity, created) = Identity::load_or_create(keystore)?;
        if created {
            tracing::info!(fingerprint = %fingerprint(&identity.identity_pk()), "identidade criada");
        }
        let endpoint = net::bind_endpoint(&cfg, &identity).await?;
        Self::start_with_endpoint(cfg, identity, keystore.describe(), endpoint)
    }

    /// Como [`Enxame::start`], mas com um endpoint já criado pelo chamador (testes com relay
    /// local, transportes customizados). O endpoint deve usar `identity.device_secret()` como
    /// chave; os ALPNs do Enxame são registrados aqui.
    pub fn start_with_endpoint(
        cfg: Config,
        identity: Identity,
        keystore_name: &'static str,
        endpoint: Endpoint,
    ) -> Result<Self> {
        if endpoint.id() != identity.endpoint_id() {
            return Err(Error::Config(
                "o endpoint não usa a chave de dispositivo da identidade".into(),
            ));
        }
        std::fs::create_dir_all(&cfg.data_dir)?;
        endpoint.set_alpns(vec![ALPN_CONTACT.to_vec(), ALPN_SYNC.to_vec()]);
        let contacts = ContactBook::open(cfg.data_dir.join("contacts.bin"))?;
        let (events, _) = broadcast::channel(64);
        let inner = Arc::new(Inner {
            cfg,
            identity,
            keystore_name,
            endpoint,
            contacts: Mutex::new(contacts),
            events,
            pending: Mutex::new(HashMap::new()),
            next_request: AtomicU64::new(1),
        });
        let accept_task = tokio::spawn(accept_loop(inner.clone()));
        Ok(Self {
            inner,
            accept_task: Arc::new(accept_task),
        })
    }

    /// Para de aceitar conexões e fecha o endpoint.
    pub async fn shutdown(&self) {
        self.accept_task.abort();
        self.inner.endpoint.close().await;
    }

    /// Chave pública de identidade.
    pub fn identity_pk(&self) -> [u8; 32] {
        self.inner.identity.identity_pk()
    }

    /// `EndpointId` (NodeId) deste dispositivo.
    pub fn endpoint_id(&self) -> EndpointId {
        self.inner.endpoint.id()
    }

    /// Endereço atual do endpoint (IPs locais, relay).
    pub fn endpoint_addr(&self) -> EndpointAddr {
        self.inner.endpoint.addr()
    }

    pub fn display_name(&self) -> &str {
        &self.inner.cfg.display_name
    }

    /// Stream de eventos. Cada chamada cria um assinante independente.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// Todos os contatos (inclusive pedidos de saída ainda pendentes).
    pub fn contacts(&self) -> Vec<Contact> {
        self.inner.book().list().to_vec()
    }

    /// Pedidos de contato aguardando resposta.
    pub fn pending_requests(&self) -> Vec<PendingRequest> {
        let mut v: Vec<_> = self
            .inner
            .pending()
            .values()
            .map(|p| p.info.clone())
            .collect();
        v.sort_by_key(|p| p.id);
        v
    }

    /// Gera um ticket de convite para este dispositivo.
    pub async fn invite_ticket(&self) -> Result<InviteTicket> {
        make_ticket(&self.inner, self.inner.cfg.ticket_ttl_secs).await
    }

    /// Importa um ticket, envia o pedido de contato e espera a decisão do outro lado.
    pub async fn add_contact(&self, ticket: &str) -> Result<AddOutcome> {
        let inner = &self.inner;
        let ticket: InviteTicket = ticket.parse()?;
        ticket.verify(unix_now())?;
        if ticket.identity_pk == inner.identity.identity_pk() {
            return Err(Error::Contact("esse ticket é seu".into()));
        }
        if let Some(c) = inner.book().by_identity(&ticket.identity_pk) {
            if c.is_mutual() {
                return Ok(AddOutcome::AlreadyContact(c.clone()));
            }
        }
        let contact = contact_from_ticket(&ticket, ContactStatus::Outgoing);
        inner.book().upsert(contact)?;

        let ours = make_ticket(inner, inner.cfg.ticket_ttl_secs).await?;
        let max = inner.cfg.max_frame_bytes;
        let wait = Duration::from_secs(inner.cfg.contact_request_timeout_secs);

        let conn = inner
            .endpoint
            .connect(ticket.node_addr.clone(), ALPN_CONTACT)
            .await
            .map_err(|e| Error::Net(format!("conectar: {e}")))?;
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| Error::Net(format!("abrir stream: {e}")))?;
        proto::write_frame(
            &mut send,
            &ContactMsg::Request {
                ticket: Box::new(ours),
            },
            max,
        )
        .await?;
        let _ = send.finish();
        let reply: ContactMsg = tokio::time::timeout(wait, proto::read_frame(&mut recv, max))
            .await
            .map_err(|_| Error::Timeout("o outro lado não respondeu ao pedido de contato"))??;
        conn.close(0u32.into(), b"ok");
        match reply {
            ContactMsg::Accept => {
                let contact = contact_from_ticket(&ticket, ContactStatus::Mutual);
                inner.book().upsert(contact.clone())?;
                inner.emit(Event::ContactAdded {
                    display_name: contact.display_name.clone(),
                    fingerprint: contact.fingerprint(),
                });
                Ok(AddOutcome::Accepted(contact))
            }
            ContactMsg::Reject => {
                inner.book().remove(&ticket.identity_pk)?;
                Ok(AddOutcome::Rejected)
            }
            ContactMsg::Request { .. } => Err(Error::Protocol("resposta inesperada".into())),
        }
    }

    /// Responde a um pedido anunciado por [`Event::ContactRequested`].
    pub fn respond_contact(&self, request_id: u64, accept: bool) -> Result<()> {
        let entry = self
            .inner
            .pending()
            .remove(&request_id)
            .ok_or_else(|| Error::Contact(format!("pedido {request_id} não existe mais")))?;
        // O handler pode já ter desistido (conexão caiu): não é erro do usuário.
        let _ = entry.decide.send(accept);
        Ok(())
    }

    /// Diagnóstico: NAT, papel e, por contato mútuo (ou o indicado), caminho e RTT.
    pub async fn diag(&self, contact: Option<&str>, direct_wait: Duration) -> Result<DiagReport> {
        let inner = &self.inner;
        let targets: Vec<Contact> = match contact {
            Some(q) => vec![inner.book().resolve(q)?.clone()],
            None => inner
                .book()
                .list()
                .iter()
                .filter(|c| c.is_mutual())
                .cloned()
                .collect(),
        };
        let nat = net::nat_report(&inner.endpoint, NAT_TIMEOUT).await;
        let mut peers = Vec::new();
        for c in targets {
            peers.push(self.diag_peer(&c, direct_wait).await);
        }
        Ok(DiagReport {
            endpoint_id: inner.endpoint.id(),
            addr: inner.endpoint.addr(),
            nat,
            role: Role::Leaf,
            keystore: inner.keystore_name,
            peers,
        })
    }

    async fn diag_peer(&self, c: &Contact, direct_wait: Duration) -> PeerDiag {
        let mut out = PeerDiag {
            name: c.display_name.clone(),
            fingerprint: c.fingerprint(),
            error: None,
            connect_time: None,
            peer_role: None,
            protocol_version: None,
            metrics: None,
        };
        if !c.is_mutual() {
            out.error = Some("contato ainda não é mútuo".into());
            return out;
        }
        let started = Instant::now();
        let conn = match dial_sync(&self.inner, c).await {
            Ok(conn) => conn,
            Err(e) => {
                out.error = Some(e.to_string());
                return out;
            }
        };
        // O monitor parte antes do Hello para enxergar a troca relay → direto desde o início.
        let mut monitor = PathMonitor::spawn(conn.clone(), started);
        match hello_exchange(&self.inner, &conn, c).await {
            Ok(hello) => {
                out.connect_time = Some(started.elapsed());
                out.peer_role = Some(hello.role);
                out.protocol_version = hello.negotiate(SUPPORTED_VERSIONS);
            }
            Err(e) => {
                out.error = Some(e.to_string());
                conn.close(1u32.into(), b"hello failed");
                return out;
            }
        }
        out.metrics = Some(monitor.wait_direct(direct_wait).await);
        conn.close(0u32.into(), b"diag done");
        out
    }
}

/// Lê a agenda sem subir a rede (para `contacts`).
pub fn list_contacts(cfg: &Config) -> Result<Vec<Contact>> {
    Ok(ContactBook::open(cfg.data_dir.join("contacts.bin"))?
        .list()
        .to_vec())
}

fn contact_from_ticket(t: &InviteTicket, status: ContactStatus) -> Contact {
    Contact {
        identity_pk: t.identity_pk,
        enc_pk: t.enc_pk,
        device_cert: t.device_cert.clone(),
        node_addr: t.node_addr.clone(),
        display_name: t.display_name.clone(),
        status,
        added_at: unix_now(),
    }
}

async fn make_ticket(inner: &Inner, ttl: u64) -> Result<InviteTicket> {
    // Com relay configurado, espera o endpoint se registrar para que o ticket o inclua.
    if inner.cfg.relay != crate::RelaySetting::Disabled {
        let _ = tokio::time::timeout(ONLINE_TIMEOUT, inner.endpoint.online()).await;
    }
    inner.identity.invite(
        inner.endpoint.addr(),
        &inner.cfg.display_name,
        unix_now(),
        ttl,
    )
}

/// Disca `enxame/sync/1` (sem trocar nada ainda).
async fn dial_sync(inner: &Inner, c: &Contact) -> Result<Connection> {
    inner
        .endpoint
        .connect(c.node_addr.clone(), ALPN_SYNC)
        .await
        .map_err(|e| Error::Net(format!("conectar: {e}")))
}

/// Troca `Hello` numa conexão `enxame/sync/1` já discada, validando o certificado do par.
async fn hello_exchange(inner: &Inner, conn: &Connection, c: &Contact) -> Result<Hello> {
    let max = inner.cfg.max_frame_bytes;
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| Error::Net(format!("abrir stream: {e}")))?;
    proto::write_frame(&mut send, &inner.our_hello(), max).await?;
    let hello: Hello = tokio::time::timeout(HANDSHAKE_TIMEOUT, proto::read_frame(&mut recv, max))
        .await
        .map_err(|_| Error::Timeout("Hello"))??;
    check_hello(&hello, c)?;
    Ok(hello)
}

fn check_hello(hello: &Hello, c: &Contact) -> Result<()> {
    hello.device_cert.verify()?;
    if hello.device_cert.identity_pk != c.identity_pk
        || hello.device_cert.node_id != *c.node_addr.id.as_bytes()
    {
        return Err(Error::Protocol(
            "Hello de outra identidade/dispositivo".into(),
        ));
    }
    if hello.negotiate(SUPPORTED_VERSIONS).is_none() {
        return Err(Error::Protocol(
            "nenhuma versão de protocolo em comum".into(),
        ));
    }
    Ok(())
}

async fn accept_loop(inner: Arc<Inner>) {
    while let Some(incoming) = inner.endpoint.accept().await {
        let inner = inner.clone();
        tokio::spawn(async move {
            let conn = match incoming.accept() {
                Ok(accepting) => match accepting.await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::debug!("handshake falhou: {e}");
                        return;
                    }
                },
                Err(e) => {
                    tracing::debug!("conexão recusada: {e}");
                    return;
                }
            };
            let result = match conn.alpn() {
                a if a == ALPN_CONTACT => handle_contact(&inner, &conn).await,
                a if a == ALPN_SYNC => handle_sync(&inner, &conn).await,
                _ => Ok(()),
            };
            if let Err(e) = result {
                tracing::debug!(peer = %conn.remote_id().fmt_short(), "conexão encerrada com erro: {e}");
                conn.close(1u32.into(), b"error");
            }
        });
    }
}

async fn handle_sync(inner: &Inner, conn: &Connection) -> Result<()> {
    let remote = conn.remote_id();
    let contact = inner.book().mutual_by_endpoint(&remote).cloned();
    let Some(contact) = contact else {
        // Desconhecido ou não mútuo: nenhum tráfego além do pedido de contato (spec 6.2).
        conn.close(CLOSE_CODE_NOT_CONTACT.into(), b"not a mutual contact");
        return Ok(());
    };
    let max = inner.cfg.max_frame_bytes;
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|e| Error::Net(format!("aceitar stream: {e}")))?;
    let hello: Hello = tokio::time::timeout(HANDSHAKE_TIMEOUT, proto::read_frame(&mut recv, max))
        .await
        .map_err(|_| Error::Timeout("Hello"))??;
    check_hello(&hello, &contact)?;
    proto::write_frame(&mut send, &inner.our_hello(), max).await?;
    // Mantém a conexão até o par fechar (o `diag` observa os caminhos nesse intervalo).
    let _ = conn.closed().await;
    Ok(())
}

async fn handle_contact(inner: &Inner, conn: &Connection) -> Result<()> {
    let remote = conn.remote_id();
    let max = inner.cfg.max_frame_bytes;
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|e| Error::Net(format!("aceitar stream: {e}")))?;
    let msg: ContactMsg =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, proto::read_frame(&mut recv, max))
            .await
            .map_err(|_| Error::Timeout("pedido de contato"))??;
    let ContactMsg::Request { ticket } = msg else {
        return Err(Error::Protocol("esperado Request".into()));
    };
    let ticket = *ticket;
    ticket.verify(unix_now())?;
    if ticket.node_addr.id != remote {
        return Err(Error::Protocol("ticket de outro dispositivo".into()));
    }
    if ticket.identity_pk == inner.identity.identity_pk() {
        return Err(Error::Protocol("pedido da própria identidade".into()));
    }

    // Quem já está na agenda (inclusive por pedido nosso) e vem do mesmo dispositivo já deu o
    // consentimento dos dois lados: aceita sem perguntar.
    let known = inner
        .book()
        .by_identity(&ticket.identity_pk)
        .is_some_and(|c| c.node_addr.id == remote);
    let accept = if known {
        true
    } else {
        ask_user(inner, conn, &ticket).await
    };

    let reply = if accept {
        let was_mutual = inner
            .book()
            .by_identity(&ticket.identity_pk)
            .is_some_and(|c| c.is_mutual());
        let contact = contact_from_ticket(&ticket, ContactStatus::Mutual);
        inner.book().upsert(contact.clone())?;
        if !was_mutual {
            inner.emit(Event::ContactAdded {
                display_name: contact.display_name.clone(),
                fingerprint: contact.fingerprint(),
            });
        }
        ContactMsg::Accept
    } else {
        ContactMsg::Reject
    };
    proto::write_frame(&mut send, &reply, max).await?;
    let _ = send.finish();
    // Dá tempo de o par ler a resposta antes de derrubar a conexão.
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
    Ok(())
}

/// Publica o pedido e espera a decisão do usuário, a queda da conexão ou o timeout.
async fn ask_user(inner: &Inner, conn: &Connection, ticket: &InviteTicket) -> bool {
    let (tx, rx) = oneshot::channel();
    let id = inner.next_request.fetch_add(1, Ordering::Relaxed);
    let info = PendingRequest {
        id,
        display_name: ticket.display_name.clone(),
        fingerprint: fingerprint(&ticket.identity_pk),
    };
    {
        let mut pending = inner.pending();
        if pending.len() >= MAX_PENDING_REQUESTS {
            return false;
        }
        pending.insert(
            id,
            PendingEntry {
                info: info.clone(),
                decide: tx,
            },
        );
    }
    inner.emit(Event::ContactRequested {
        request_id: id,
        display_name: info.display_name,
        fingerprint: info.fingerprint,
    });
    let wait = Duration::from_secs(inner.cfg.contact_request_timeout_secs);
    let decision = tokio::select! {
        d = rx => d.unwrap_or(false),
        _ = tokio::time::sleep(wait) => false,
        _ = conn.closed() => false,
    };
    inner.pending().remove(&id);
    decision
}
