//! `Enxame`: ponto de entrada da API do core (spec 3.1).

mod carry_flow;
mod files;
mod runtime;

pub use files::{Downloaded, FileEntry, PrefetchedBlob};

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

use iroh::{
    endpoint::{Connection, RecvStream, SendStream},
    protocol::ProtocolHandler,
    Endpoint, EndpointAddr, EndpointId,
};
use tokio::{
    sync::{broadcast, oneshot, watch},
    task::JoinHandle,
};

use crate::{
    carry::{make_receipt, RECEIPT_TTL},
    config::{Config, RoleMode},
    contacts::{self, Contact, ContactBook, ContactStatus},
    crypto::{dm_conversation_id, Envelope, EnvelopeKind, Recipient, MAX_TEXT_BYTES},
    hlc::{Hlc, HlcClock},
    identity::{fingerprint, unix_now, unix_now_ms, Identity, InviteTicket},
    keystore::KeyStore,
    net::{self, NatKind, NatReport, PathMetrics, PathMonitor},
    order::order_ids,
    proto::{self, ContactMsg, Hello, ALPN_CARRY, ALPN_CONTACT, ALPN_SYNC, SUPPORTED_VERSIONS},
    roles::{effective_kbps_cap, PlatformSignals, RateLimiter, RoleEngine, ThroughputEstimator},
    store::{self, Store},
    sync::{common_hash, Session, SyncStats},
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
    /// Papel atual deste nó.
    pub role: Role,
    pub keystore: &'static str,
    pub peers: Vec<PeerDiag>,
}

/// Mensagem de uma conversa, pronta para exibir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: [u8; 32],
    pub from_me: bool,
    pub author_name: String,
    pub hlc: Hlc,
    pub text: String,
    /// Quando este dispositivo guardou a mensagem (ms desde a época Unix).
    pub received_at_ms: u64,
    /// Só para mensagens minhas: o destinatário já a recebeu (direto ou por recibo de um
    /// carrier). `None` nas mensagens recebidas.
    pub delivered: Option<bool>,
    /// Quando a mensagem é um arquivo (`text` traz uma descrição "📎 nome (tamanho)").
    pub file: Option<MessageFile>,
}

/// Arquivo anexado a uma mensagem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageFile {
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub blob: [u8; 32],
}

/// Resultado de [`Enxame::send_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentFile {
    /// Uma mensagem por destinatário, na ordem em que foram pedidos.
    pub messages: Vec<SentMessage>,
    pub name: String,
    pub size: u64,
    /// Raiz do blob (id do arquivo na rede).
    pub blob: [u8; 32],
}

/// Opções de inicialização para integradores e testes.
#[derive(Default)]
pub struct StartOptions {
    /// Fonte de sinais do SO. Padrão: Linux ou, nas outras plataformas, só CPU/memória.
    pub signals: Option<Arc<dyn PlatformSignals>>,
}

/// Quanto este nó serviu a terceiros no mês e o que guarda para entregar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributionSummary {
    /// Mês (`AAAAMM`, UTC).
    pub month: u32,
    pub bytes_served: u64,
    pub items_delivered: u64,
    /// Teto mensal configurado, em bytes.
    pub budget_bytes: u64,
    /// `(contato, bytes, itens)` do mês.
    pub by_contact: Vec<(String, u64, u64)>,
    /// Envelopes de terceiros guardados para entregar.
    pub carry_items: u64,
    pub carry_bytes: u64,
}

/// Resultado de [`Enxame::send_text`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMessage {
    pub id: [u8; 32],
    /// O par estava online e a sessão de sincronização terminou. Se `false`, a mensagem está
    /// guardada e segue no próximo contato (`error` diz por que a sessão falhou).
    pub synced: bool,
    pub error: Option<String>,
    /// Contatos em comum que aceitaram carregar a mensagem até o destinatário voltar.
    pub carriers: usize,
}

struct PendingEntry {
    info: PendingRequest,
    decide: oneshot::Sender<bool>,
}

struct Inner {
    blobs: files::BlobsRt,
    /// Fila de blobs a replicar como carrier.
    prefetch_tx: tokio::sync::mpsc::UnboundedSender<files::PrefetchJob>,
    /// Bytes de blobs enviados a contatos (inclui os meus próprios arquivos).
    files_sent_bytes: AtomicU64,
    cfg: Config,
    identity: Identity,
    keystore_name: &'static str,
    endpoint: Endpoint,
    contacts: Mutex<ContactBook>,
    store: Store,
    clock: Mutex<HlcClock>,
    active_syncs: Mutex<HashSet<[u8; 32]>>,
    events: broadcast::Sender<Event>,
    pending: Mutex<HashMap<u64, PendingEntry>>,
    next_request: AtomicU64,
    signals: Arc<dyn PlatformSignals>,
    throughput: ThroughputEstimator,
    role_mode: RoleMode,
    engine: Mutex<RoleEngine>,
    role_tx: watch::Sender<Role>,
    game_mode: AtomicBool,
    contribute: AtomicBool,
    nat: Mutex<NatKind>,
    started: Instant,
    limiter: Mutex<(Option<u64>, Arc<RateLimiter>)>,
}

impl Inner {
    fn book(&self) -> MutexGuard<'_, ContactBook> {
        self.contacts.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn clock(&self) -> MutexGuard<'_, HlcClock> {
        self.clock.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn pending(&self) -> MutexGuard<'_, HashMap<u64, PendingEntry>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, ev: Event) {
        // Sem assinantes não é erro: o core não presume UI ativa.
        let _ = self.events.send(ev);
    }

    fn role(&self) -> Role {
        *self.role_tx.borrow()
    }

    fn our_hello(&self) -> Hello {
        Hello {
            device_cert: self.identity.device_cert(unix_now()),
            protocol_versions: SUPPORTED_VERSIONS.to_vec(),
            role: self.role(),
            capabilities: 0,
        }
    }

    /// Limitador de banda de contribuição, reconstruído quando o teto efetivo muda.
    fn limiter(&self) -> Arc<RateLimiter> {
        let cap = effective_kbps_cap(&self.cfg.budget, self.throughput.kbps());
        let mut guard = self.limiter.lock().unwrap_or_else(|p| p.into_inner());
        if guard.0 != cap {
            *guard = (cap, Arc::new(RateLimiter::new(cap)));
        }
        guard.1.clone()
    }

    /// Ensina ao endpoint como alcançar o contato (o `Downloader` conecta só por `EndpointId`).
    fn register_addr(&self, c: &Contact) {
        self.blobs.lookup.add_endpoint_info(c.node_addr.clone());
    }

    fn mutual_contacts(&self) -> Vec<Contact> {
        self.book()
            .list()
            .iter()
            .filter(|c| c.is_mutual())
            .cloned()
            .collect()
    }
}

/// Núcleo do Enxame. Barato de clonar.
#[derive(Clone)]
pub struct Enxame {
    inner: Arc<Inner>,
    tasks: Arc<Vec<JoinHandle<()>>>,
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
        Self::start_with_options(cfg, keystore, StartOptions::default()).await
    }

    /// Como [`Enxame::start`], com [`StartOptions`].
    pub async fn start_with_options(
        cfg: Config,
        keystore: &dyn KeyStore,
        opts: StartOptions,
    ) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let (identity, created) = Identity::load_or_create(keystore)?;
        if created {
            tracing::info!(fingerprint = %fingerprint(&identity.identity_pk()), "identidade criada");
        }
        let endpoint = net::bind_endpoint(&cfg, &identity).await?;
        Self::start_with_endpoint_opts(cfg, identity, keystore.describe(), endpoint, opts).await
    }

    /// Como [`Enxame::start`], mas com um endpoint já criado pelo chamador (testes com relay
    /// local, transportes customizados). O endpoint deve usar `identity.device_secret()` como
    /// chave; os ALPNs do Enxame são registrados aqui.
    pub async fn start_with_endpoint(
        cfg: Config,
        identity: Identity,
        keystore_name: &'static str,
        endpoint: Endpoint,
    ) -> Result<Self> {
        Self::start_with_endpoint_opts(
            cfg,
            identity,
            keystore_name,
            endpoint,
            StartOptions::default(),
        )
        .await
    }

    /// Como [`Enxame::start_with_endpoint`], com [`StartOptions`].
    pub async fn start_with_endpoint_opts(
        cfg: Config,
        identity: Identity,
        keystore_name: &'static str,
        endpoint: Endpoint,
        opts: StartOptions,
    ) -> Result<Self> {
        if endpoint.id() != identity.endpoint_id() {
            return Err(Error::Config(
                "o endpoint não usa a chave de dispositivo da identidade".into(),
            ));
        }
        std::fs::create_dir_all(&cfg.data_dir)?;
        endpoint.set_alpns(vec![
            ALPN_CONTACT.to_vec(),
            ALPN_SYNC.to_vec(),
            ALPN_CARRY.to_vec(),
            iroh_blobs::ALPN.to_vec(),
        ]);
        let role_mode = RoleMode::parse(&cfg.role)?;
        let initial_role = match role_mode {
            RoleMode::Fixed(r) => r,
            RoleMode::Auto => Role::Leaf,
        };
        let signals: Arc<dyn PlatformSignals> = opts.signals.unwrap_or_else(default_signals);
        let engine = RoleEngine::new(
            Duration::from_secs(cfg.promote_after_secs),
            Duration::from_secs(cfg.promote_after_super_secs),
        );
        let (role_tx, _) = watch::channel(initial_role);
        let (game_mode, contribute) = (cfg.game_mode, cfg.contribute);
        let store = Store::open(&cfg.data_dir.join("enxame.db"))?;
        import_legacy_contacts(&cfg, &store)?;
        let contacts = ContactBook::open(store.clone())?;
        let clock = HlcClock::resume_from(store.with(|c| store::max_hlc(c))?);
        let (events, _) = broadcast::channel(64);
        let (blobs, provider_rx) = files::open_blobs(&cfg, &endpoint).await?;
        let (prefetch_tx, prefetch_rx) = tokio::sync::mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            blobs,
            prefetch_tx,
            files_sent_bytes: AtomicU64::new(0),
            cfg,
            identity,
            keystore_name,
            endpoint,
            contacts: Mutex::new(contacts),
            store,
            clock: Mutex::new(clock),
            active_syncs: Mutex::new(HashSet::new()),
            events,
            pending: Mutex::new(HashMap::new()),
            next_request: AtomicU64::new(1),
            signals,
            throughput: ThroughputEstimator::default(),
            role_mode,
            engine: Mutex::new(engine),
            role_tx,
            game_mode: AtomicBool::new(game_mode),
            contribute: AtomicBool::new(contribute),
            nat: Mutex::new(NatKind::Unknown),
            started: Instant::now(),
            limiter: Mutex::new((None, Arc::new(RateLimiter::new(None)))),
        });
        for c in inner.mutual_contacts() {
            inner.register_addr(&c);
        }
        let mut tasks = vec![
            tokio::spawn(accept_loop(inner.clone())),
            tokio::spawn(files::provider_policy(inner.clone(), provider_rx)),
            tokio::spawn(files::maintenance_loop(inner.clone())),
            tokio::spawn(files::prefetch_worker(inner.clone(), prefetch_rx)),
        ];
        if inner.cfg.sync_interval_secs > 0 {
            tasks.push(tokio::spawn(sync_loop(inner.clone())));
        }
        if inner.cfg.role_interval_secs > 0 {
            tasks.push(tokio::spawn(runtime::role_loop(inner.clone())));
            tasks.push(tokio::spawn(runtime::nat_loop(inner.clone())));
        }
        Ok(Self {
            inner,
            tasks: Arc::new(tasks),
        })
    }

    /// Papel atual deste nó.
    pub fn role(&self) -> Role {
        self.inner.role()
    }

    /// Liga ou desliga o modo jogo (força `Leaf`). O rebaixamento é imediato.
    pub async fn set_game_mode(&self, on: bool) -> Option<(Role, Role)> {
        self.inner.game_mode.store(on, Ordering::Relaxed);
        runtime::tick_roles(&self.inner, self.inner.started.elapsed()).await
    }

    /// Liga ou desliga a contribuição. Desligada, o papel é `Leaf`.
    pub async fn set_contribution(&self, on: bool) -> Option<(Role, Role)> {
        self.inner.contribute.store(on, Ordering::Relaxed);
        runtime::tick_roles(&self.inner, self.inner.started.elapsed()).await
    }

    /// Reavalia o papel como se `at` tivesse passado desde o início (relógio simulado).
    #[doc(hidden)]
    pub async fn tick_roles_at(&self, at: Duration) -> Option<(Role, Role)> {
        runtime::tick_roles(&self.inner, at).await
    }

    /// Último papel conhecido de um contato (visto no `Hello` ou em `RoleUpdate`).
    pub fn peer_role(&self, contact: &str) -> Result<Option<Role>> {
        let c = self.inner.book().resolve(contact)?.clone();
        let (_, _, role) = self
            .inner
            .store
            .with(|db| store::reliability_get(db, &c.identity_pk))?;
        Ok(role)
    }

    /// Contribuição do mês e o que está guardado para entregar a terceiros.
    pub async fn contribution_summary(&self) -> Result<ContributionSummary> {
        runtime::contribution_summary(&self.inner).await
    }

    /// Sincroniza e troca carry com um contato mútuo agora (útil em testes e na CLI).
    pub async fn carry_now(&self, contact: &str) -> Result<crate::carry::CarryStats> {
        let c = self.inner.book().resolve(contact)?.clone();
        if !c.is_mutual() {
            return Err(Error::Contact(
                "o contato ainda não aceitou (não é mútuo)".into(),
            ));
        }
        carry_flow::carry_with(&self.inner, &c, vec![]).await
    }

    /// Para de aceitar conexões e fecha o endpoint.
    pub async fn shutdown(&self) {
        for t in self.tasks.iter() {
            t.abort();
        }
        self.inner.endpoint.close().await;
        // Libera o store de blobs (a trava de arquivo do banco): sem isso um novo `start` no
        // mesmo diretório ficaria esperando.
        if let Err(e) = self.inner.blobs.store.shutdown().await {
            tracing::debug!("erro ao encerrar o store de blobs: {e}");
        }
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
                inner.register_addr(&contact);
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
            role: inner.role(),
            keystore: inner.keystore_name,
            peers,
        })
    }

    /// Envia uma mensagem de texto a um contato mútuo. A mensagem é guardada antes de qualquer
    /// tentativa de rede; se o par estiver offline ela segue na próxima sincronização.
    pub async fn send_text(&self, contact: &str, text: &str) -> Result<SentMessage> {
        if text.trim().is_empty() {
            return Err(Error::Contact("mensagem vazia".into()));
        }
        if text.len() > MAX_TEXT_BYTES {
            return Err(Error::Contact(format!(
                "mensagem acima de {MAX_TEXT_BYTES} bytes"
            )));
        }
        send_envelope(
            &self.inner,
            contact,
            EnvelopeKind::Text,
            text.as_bytes().to_vec(),
        )
        .await
    }

    /// Cifra e envia um arquivo a um contato mútuo (veja [`Enxame::send_file_to`]).
    pub async fn send_file(&self, contact: &str, path: &std::path::Path) -> Result<SentFile> {
        self.send_file_to(&[contact], path).await
    }

    /// Cifra o arquivo **uma vez** e envia o mesmo `FileRef` a vários contatos. O arquivo é
    /// cifrado em streaming, importado nos blobs e anunciado por uma mensagem `FileRef` (E2E);
    /// quem recebe baixa de todos os contatos que o tiverem, e quem já baixou vira fonte para os
    /// demais.
    pub async fn send_file_to(
        &self,
        contacts: &[&str],
        path: &std::path::Path,
    ) -> Result<SentFile> {
        let (messages, fr) = files::send_file(&self.inner, contacts, path).await?;
        Ok(SentFile {
            messages,
            name: fr.name,
            size: fr.size,
            blob: fr.blob_hash,
        })
    }

    /// Arquivos da conversa com `contact`, do mais antigo ao mais novo.
    pub async fn files(&self, contact: &str) -> Result<Vec<files::FileEntry>> {
        files::list_files(&self.inner, contact).await
    }

    /// Baixa o arquivo `which` (posição em [`Enxame::files`] ou prefixo do id) de todos os
    /// contatos que o tiverem, decifra em `dest_dir` (padrão: `download_dir` da configuração) e
    /// passa a semeá-lo. Continua de onde parou se uma tentativa anterior foi interrompida.
    pub async fn download_file(
        &self,
        contact: &str,
        which: &str,
        dest_dir: Option<std::path::PathBuf>,
    ) -> Result<files::Downloaded> {
        files::download(&self.inner, contact, which, dest_dir).await
    }

    /// Arquivos que este nó guarda como carrier (pré-replicados), com o prazo de cada um.
    pub async fn prefetched_blobs(&self) -> Result<Vec<PrefetchedBlob>> {
        files::list_prefetched(&self.inner).await
    }

    /// O blob (raiz) está completo no store local? (diagnóstico e testes)
    #[doc(hidden)]
    pub async fn has_blob(&self, hash: [u8; 32]) -> bool {
        files::has_blob(&self.inner, hash).await
    }

    /// Vence agora as cópias replicadas e roda a manutenção (testes).
    #[doc(hidden)]
    pub async fn expire_prefetched_now(&self) -> Result<()> {
        files::expire_prefetched_now(&self.inner).await
    }

    /// Bytes de arquivos enviados a contatos desde que o nó subiu (inclui os seus próprios).
    pub fn file_bytes_sent(&self) -> u64 {
        self.inner.files_sent_bytes.load(Ordering::Relaxed)
    }

    /// Sincroniza a conversa com um contato mútuo agora.
    pub async fn sync_now(&self, contact: &str) -> Result<SyncStats> {
        let c = self.inner.book().resolve(contact)?.clone();
        if !c.is_mutual() {
            return Err(Error::Contact(
                "o contato ainda não aceitou (não é mútuo)".into(),
            ));
        }
        sync_with(&self.inner, &c).await
    }

    /// Últimas `limit` mensagens da conversa com `contact`, na ordem de exibição (topológica
    /// por `parents`, desempate por HLC e id).
    pub async fn history(&self, contact: &str, limit: usize) -> Result<Vec<Message>> {
        let inner = &self.inner;
        let c = inner.book().resolve(contact)?.clone();
        build_history(
            &inner.store,
            &inner.identity.identity_pk(),
            &inner.cfg,
            &c,
            limit,
        )
        .await
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
            Ok((hello, _send, _recv)) => {
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

/// Cria, guarda e entrega um envelope 1:1 (texto ou `FileRef`) a um contato mútuo. O envelope é
/// guardado antes de qualquer tentativa de rede; com o contato offline, pede carriers.
async fn send_envelope(
    inner: &Inner,
    contact: &str,
    kind: EnvelopeKind,
    plain: Vec<u8>,
) -> Result<SentMessage> {
    let c = inner.book().resolve(contact)?.clone();
    if !c.is_mutual() {
        return Err(Error::Contact(
            "o contato ainda não aceitou (não é mútuo)".into(),
        ));
    }
    let me = inner.identity.identity_pk();
    let conv = dm_conversation_id(&me, &c.identity_pk);
    let parents = inner.store.run(move |db| store::heads(db, &conv)).await?;
    let hlc = inner.clock().tick(unix_now_ms());
    let env = Envelope::create(
        &inner.identity,
        conv,
        hlc,
        parents,
        &[
            Recipient {
                identity_pk: c.identity_pk,
                enc_pk: c.enc_pk,
            },
            Recipient {
                identity_pk: me,
                enc_pk: inner.identity.enc_pk(),
            },
        ],
        kind,
        &plain,
    )?;
    let id = env.id;
    let plain_store = plain.clone();
    let cpk = c.identity_pk;
    inner
        .store
        .run(move |db| {
            store::envelope_insert(db, &env, Some(&plain_store), true, unix_now_ms())?;
            store::pending_add(db, &id, &cpk, unix_now_ms())
        })
        .await?;
    match sync_with(inner, &c).await {
        Ok(_) => Ok(SentMessage {
            id,
            synced: true,
            error: None,
            carriers: 0,
        }),
        Err(e) => {
            tracing::debug!("mensagem guardada, par inacessível: {e}");
            // Contato offline: pede a contatos em comum que carreguem até ele voltar.
            let carriers = carry_flow::offer_to_carriers(inner, &c).await.unwrap_or(0);
            Ok(SentMessage {
                id,
                synced: false,
                error: Some(e.to_string()),
                carriers,
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn default_signals() -> Arc<dyn PlatformSignals> {
    Arc::new(crate::roles::LinuxSignals::new())
}

#[cfg(not(target_os = "linux"))]
fn default_signals() -> Arc<dyn PlatformSignals> {
    Arc::new(crate::roles::FallbackSignals::new())
}

/// Lê a agenda sem subir a rede (para `contacts`).
pub fn list_contacts(cfg: &Config) -> Result<Vec<Contact>> {
    let store = Store::open(&cfg.data_dir.join("enxame.db"))?;
    import_legacy_contacts(cfg, &store)?;
    Ok(ContactBook::open(store)?.list().to_vec())
}

/// Histórico de uma conversa lido direto do banco, **sem** subir o nó (sem endpoint nem relay).
/// Serve para `history` com outro processo (chat, serve) rodando na mesma identidade: abrir um
/// segundo endpoint com a mesma chave derrubaria o primeiro no relay.
pub async fn read_history(
    cfg: &Config,
    keystore: &dyn KeyStore,
    contact: &str,
    limit: usize,
) -> Result<Vec<Message>> {
    let (identity, _) = Identity::load_or_create(keystore)?;
    let store = Store::open(&cfg.data_dir.join("enxame.db"))?;
    import_legacy_contacts(cfg, &store)?;
    let book = ContactBook::open(store.clone())?;
    let c = book.resolve(contact)?.clone();
    build_history(&store, &identity.identity_pk(), cfg, &c, limit).await
}

async fn build_history(
    store: &Store,
    my_pk: &[u8; 32],
    cfg: &Config,
    c: &Contact,
    limit: usize,
) -> Result<Vec<Message>> {
    let conv = dm_conversation_id(my_pk, &c.identity_pk);
    let cpk = c.identity_pk;
    let (all, pending) = store
        .run(move |db| {
            let all = store::conversation_envelopes(db, &conv)?;
            let pending: HashSet<[u8; 32]> = store::pending_list(db, &cpk)?.into_iter().collect();
            Ok((all, pending))
        })
        .await?;
    let order = order_ids(&all.iter().map(|s| s.order_item()).collect::<Vec<_>>());
    let by_id: HashMap<[u8; 32], &store::StoredEnvelope> =
        all.iter().map(|s| (s.envelope.id, s)).collect();
    let mut out: Vec<Message> = order
        .iter()
        .filter_map(|id| by_id.get(id))
        .filter(|s| matches!(s.envelope.kind, EnvelopeKind::Text | EnvelopeKind::FileRef))
        .map(|s| {
            // Arquivo: a descrição vem do `FileRef` (a chave nunca sai desta função).
            let file = (s.envelope.kind == EnvelopeKind::FileRef)
                .then(|| {
                    let max = cfg.max_file_mb.saturating_mul(1024 * 1024);
                    crate::FileRef::decode(s.plaintext.as_deref().unwrap_or_default(), max).ok()
                })
                .flatten();
            (s, file)
        })
        .map(|(s, file)| Message {
            id: s.envelope.id,
            from_me: s.outgoing,
            author_name: if s.outgoing {
                cfg.display_name.clone()
            } else {
                c.display_name.clone()
            },
            hlc: s.envelope.hlc,
            text: match &file {
                Some(f) => f.describe(),
                None => {
                    String::from_utf8_lossy(s.plaintext.as_deref().unwrap_or_default()).into_owned()
                }
            },
            received_at_ms: s.received_at,
            delivered: s.outgoing.then(|| !pending.contains(&s.envelope.id)),
            file: file.map(|f| MessageFile {
                name: crate::sanitize_name(&f.name),
                size: f.size,
                mime: f.mime,
                blob: f.blob_hash,
            }),
        })
        .collect();
    let skip = out.len().saturating_sub(limit);
    out.drain(..skip);
    Ok(out)
}

/// Importa o `contacts.bin` da Fase 1 para o banco, uma única vez.
fn import_legacy_contacts(cfg: &Config, store: &Store) -> Result<()> {
    let path = cfg.data_dir.join("contacts.bin");
    if let Some(legacy) = contacts::read_legacy(&path)? {
        store.with(|c| {
            for contact in &legacy {
                store::contact_upsert(c, contact)?;
            }
            Ok(())
        })?;
        std::fs::rename(&path, cfg.data_dir.join("contacts.bin.migrated"))?;
        tracing::info!(
            n = legacy.len(),
            "contatos da Fase 1 importados para o banco"
        );
    }
    Ok(())
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

/// Troca `Hello` numa conexão `enxame/sync/1` já discada, validando o certificado do par. Os
/// streams voltam para continuar a sessão de sincronização no mesmo canal.
async fn hello_exchange(
    inner: &Inner,
    conn: &Connection,
    c: &Contact,
) -> Result<(Hello, SendStream, RecvStream)> {
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
    Ok((hello, send, recv))
}

/// Roda a sessão de sincronização com `c` sobre um par de streams e, se terminar bem: limpa as
/// minhas mensagens que o par acabou de receber, emite recibos pelo que recebi e grava os
/// contatos em comum.
async fn run_session(
    inner: &Inner,
    c: &Contact,
    recv: RecvStream,
    send: &mut SendStream,
) -> Result<SyncStats> {
    let me = inner.identity.identity_pk();
    let conv = dm_conversation_id(&me, &c.identity_pk);
    let peer_pk = c.identity_pk;
    let name = c.display_name.clone();
    let peer_fp = c.fingerprint();
    let stored_ids: Mutex<Vec<[u8; 32]>> = Mutex::new(Vec::new());
    let common_in: Mutex<Vec<[u8; 32]>> = Mutex::new(Vec::new());
    let on_stored = |env: &Envelope| {
        if env.author != me {
            stored_ids
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(env.id);
            inner.emit(Event::MessageReceived {
                conversation_id: env.conversation_id,
                id: env.id,
                from_name: name.clone(),
                from_fingerprint: peer_fp.clone(),
            });
        }
        inner.clock().observe(env.hlc, unix_now_ms());
    };
    let on_common = |hashes: Vec<[u8; 32]>| {
        *common_in.lock().unwrap_or_else(|p| p.into_inner()) = hashes;
    };
    let on_role = |role: Role| {
        let _ = inner
            .store
            .with(|db| store::reliability_set_role(db, &peer_pk, role, unix_now_ms()));
    };
    let mutual = inner.mutual_contacts();
    let common_hashes: Vec<[u8; 32]> = mutual
        .iter()
        .filter(|x| x.identity_pk != peer_pk)
        .map(|x| common_hash(&conv, &x.identity_pk))
        .collect();
    let snapshot = inner
        .store
        .run(move |db| store::pending_list(db, &peer_pk))
        .await?;
    let session = Session {
        store: &inner.store,
        identity: &inner.identity,
        peer_pk,
        conversations: vec![conv],
        max_frame: inner.cfg.max_frame_bytes,
        on_stored: &on_stored,
        max_file_bytes: inner.cfg.max_file_mb.saturating_mul(1024 * 1024),
        common_hashes,
        on_common: &on_common,
        on_role: &on_role,
        role_updates: Some(inner.role_tx.subscribe()),
    };
    let started = Instant::now();
    let stats = session.run(recv, send).await?;
    // Banda realmente usada para enviar: alimenta a estimativa passiva de upload.
    inner.throughput.record(stats.sent_bytes, started.elapsed());

    // O par terminou a sessão tendo recebido tudo o que eu tinha: nada mais está pendente.
    inner
        .store
        .run(move |db| store::pending_remove(db, &snapshot, &peer_pk))
        .await?;

    // Recibos pelo que recebi do par (para carriers que ainda guardam cópia expurgarem).
    let ids = std::mem::take(&mut *stored_ids.lock().unwrap_or_else(|p| p.into_inner()));
    for chunk in ids.chunks(crate::crypto::MAX_PARENTS) {
        let hlc = inner.clock().tick(unix_now_ms());
        let receipt = make_receipt(&inner.identity, c, chunk, hlc)?;
        let (now, ttl) = (unix_now_ms(), RECEIPT_TTL.as_millis() as u64);
        inner
            .store
            .run(move |db| store::receipt_store(db, &receipt, now, ttl))
            .await?;
    }

    // Contatos em comum com este par.
    let theirs: HashSet<[u8; 32]> =
        std::mem::take(&mut *common_in.lock().unwrap_or_else(|p| p.into_inner()))
            .into_iter()
            .collect();
    let others: Vec<[u8; 32]> = mutual
        .iter()
        .filter(|x| {
            x.identity_pk != peer_pk && theirs.contains(&common_hash(&conv, &x.identity_pk))
        })
        .map(|x| x.identity_pk)
        .collect();
    inner
        .store
        .run(move |db| store::common_set(db, &peer_pk, &others))
        .await?;
    Ok(stats)
}

/// Disca o contato, troca `Hello` e sincroniza a conversa.
async fn sync_with(inner: &Inner, c: &Contact) -> Result<SyncStats> {
    let pk = c.identity_pk;
    let dial = tokio::time::timeout(
        Duration::from_secs(inner.cfg.dial_timeout_secs),
        dial_sync(inner, c),
    )
    .await
    .map_err(|_| Error::Timeout("conectar ao contato"))
    .and_then(|r| r);
    let conn = match dial {
        Ok(conn) => conn,
        Err(e) => {
            let _ = inner
                .store
                .run(move |db| store::reliability_record(db, &pk, false, None, unix_now_ms()))
                .await;
            return Err(e);
        }
    };
    let result = async {
        let (hello, mut send, recv) = hello_exchange(inner, &conn, c).await?;
        let role = hello.role;
        inner
            .store
            .run(move |db| store::reliability_record(db, &pk, true, Some(role), unix_now_ms()))
            .await?;
        let stats = run_session(inner, c, recv, &mut send).await?;
        let _ = send.finish();
        Ok(stats)
    }
    .await;
    conn.close(0u32.into(), b"sync done");
    result
}

/// Guarda de sessão automática por contato: evita sessões automáticas concorrentes.
struct SyncGuard {
    inner: Arc<Inner>,
    pk: [u8; 32],
}

impl SyncGuard {
    fn try_new(inner: &Arc<Inner>, pk: [u8; 32]) -> Option<Self> {
        let mut active = inner.active_syncs.lock().unwrap_or_else(|p| p.into_inner());
        active.insert(pk).then(|| Self {
            inner: inner.clone(),
            pk,
        })
    }
}

impl Drop for SyncGuard {
    fn drop(&mut self) {
        let mut active = self
            .inner
            .active_syncs
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        active.remove(&self.pk);
    }
}

/// Sincroniza periodicamente com todos os contatos mútuos (entrega mensagens pendentes e puxa
/// as que faltam). Cada contato tem a sua tarefa, para um par offline não atrasar os outros.
async fn sync_loop(inner: Arc<Inner>) {
    let mut tick = tokio::time::interval(Duration::from_secs(inner.cfg.sync_interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let contacts: Vec<Contact> = inner
            .book()
            .list()
            .iter()
            .filter(|c| c.is_mutual())
            .cloned()
            .collect();
        for c in contacts {
            let Some(guard) = SyncGuard::try_new(&inner, c.identity_pk) else {
                continue;
            };
            let inner = inner.clone();
            tokio::spawn(async move {
                let _guard = guard;
                match sync_with(&inner, &c).await {
                    // Contato online: aproveita para entregar o que carrego e trocar recibos.
                    Ok(_) => {
                        if let Err(e) = carry_flow::carry_with(&inner, &c, vec![]).await {
                            tracing::debug!(peer = %c.fingerprint(), "carry falhou: {e}");
                        }
                    }
                    // Contato offline: se há mensagens pendentes, procura carriers em comum.
                    Err(e) => {
                        tracing::debug!(peer = %c.fingerprint(), "sincronização automática falhou: {e}");
                        if let Err(e) = carry_flow::offer_to_carriers(&inner, &c).await {
                            tracing::debug!(peer = %c.fingerprint(), "ofertas de carry falharam: {e}");
                        }
                    }
                }
            });
        }
    }
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
                a if a == ALPN_CARRY => carry_flow::handle_carry(&inner, &conn).await,
                a if a == iroh_blobs::ALPN => inner
                    .blobs
                    .protocol
                    .accept(conn.clone())
                    .await
                    .map_err(|e| Error::Net(format!("blobs: {e}"))),
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
    // Segue com a sessão de sincronização no mesmo stream. Se o par só queria o `Hello`
    // (`diag`), ele fecha a conexão e a leitura falha, o que é normal.
    let stats = run_session(inner, &contact, recv, &mut send).await?;
    let _ = send.finish();
    tracing::debug!(?stats, peer = %contact.fingerprint(), "sessão de sincronização recebida");
    // Dá tempo de o iniciador fechar a conexão depois de ler o nosso `Bye`.
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
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
    let allowed = inner.cfg.auto_accept_contacts
        || inner
            .cfg
            .allowed_fingerprints
            .iter()
            .any(|f| f.eq_ignore_ascii_case(&fingerprint(&ticket.identity_pk)));
    let accept = if known || allowed {
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
        inner.register_addr(&contact);
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
