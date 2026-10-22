//! Arquivos no nó: serviço de blobs, política de quem pode baixar, envio, download e coleta.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use iroh::{address_lookup::MemoryLookup, Endpoint, EndpointId};
use iroh_blobs::{
    api::downloader::Downloader,
    provider::events::{
        AbortReason, ConnectMode, EventMask, EventSender, ProviderMessage, RequestMode,
        ThrottleMode,
    },
    store::{
        fs::{options::Options as FsOptions, FsStore},
        GcConfig,
    },
    BlobsProtocol, Hash, HashAndFormat,
};
use rand::RngCore;
use tokio::sync::mpsc;

use super::{Inner, SentMessage};
use crate::{
    blobs,
    carry::current_month,
    contacts::Contact,
    crypto::{dm_conversation_id, EnvelopeKind},
    files::{guess_mime, sanitize_name, FileRef},
    identity::unix_now_ms,
    store::{self, BlobOrigin, BlobRow},
    Error, Event, Result, Role,
};
use crate::{proto::BlobOffer, roles::CarryLimits};

const MAINTENANCE_EVERY: Duration = Duration::from_secs(60);
const PROGRESS_MIN_STEP: u64 = 256 * 1024;

/// Passo mínimo entre eventos de progresso: no máximo ~100 eventos por arquivo.
fn progress_step(total: u64) -> u64 {
    (total / 100).max(PROGRESS_MIN_STEP)
}

/// Serviço de blobs de um nó.
pub(super) struct BlobsRt {
    pub store: FsStore,
    pub protocol: BlobsProtocol,
    pub downloader: Downloader,
    /// Agenda de endereços dos contatos, para o `Downloader` achar quem só conhece por `EndpointId`.
    pub lookup: MemoryLookup,
}

fn blob_err(e: impl std::fmt::Display) -> Error {
    Error::Blob(e.to_string())
}

/// Nome da tag que protege a raiz `hash` da coleta de lixo.
pub(super) fn tag_name(hash: &Hash) -> String {
    format!("enx/{}", hash.to_hex())
}

/// Abre o store em `<data_dir>/blobs` (com GC periódica) e monta o protocolo e o downloader.
/// Devolve também o canal de eventos do provedor, que a política consome.
pub(super) async fn open_blobs(
    cfg: &crate::Config,
    endpoint: &Endpoint,
) -> Result<(BlobsRt, mpsc::Receiver<ProviderMessage>)> {
    let root = cfg.data_dir.join("blobs");
    std::fs::create_dir_all(&root)?;
    let mut opts = FsOptions::new(&root);
    opts.gc = Some(GcConfig {
        interval: Duration::from_secs(cfg.blob_gc_interval_secs.max(1)),
        add_protected: None,
    });
    let store = FsStore::load_with_opts(root.join("blobs.db"), opts)
        .await
        .map_err(blob_err)?;
    let mask = EventMask {
        connected: ConnectMode::Intercept,
        get: RequestMode::Intercept,
        // `get_many` e `push` ficam desligados: nada de ler hashes por atalho nem escrever aqui.
        get_many: RequestMode::Disabled,
        throttle: ThrottleMode::Intercept,
        ..EventMask::DEFAULT
    };
    let (events, rx) = EventSender::channel(64, mask);
    let protocol = BlobsProtocol::new(&store, Some(events));
    let downloader = Downloader::new(&store, endpoint);
    let lookup = MemoryLookup::new();
    if let Ok(services) = endpoint.address_lookup() {
        services.add(lookup.clone());
    }
    Ok((
        BlobsRt {
            store,
            protocol,
            downloader,
            lookup,
        },
        rx,
    ))
}

/// O orçamento mensal de contribuição já foi gasto?
async fn budget_exceeded(inner: &Inner) -> bool {
    let month = current_month();
    let used = inner
        .store
        .run(move |c| store::contribution_month_bytes(c, month))
        .await
        .unwrap_or(0);
    used >= inner
        .cfg
        .budget
        .max_contrib_gb_month
        .saturating_mul(1_000_000_000)
}

struct ReqState {
    pk: [u8; 32],
    counts: bool,
}

/// Política do provedor: só contatos mútuos, só o que o papel e o budget permitem, com limite
/// de banda e contabilidade. Roda como tarefa de fundo consumindo os eventos do `iroh-blobs`.
pub(super) async fn provider_policy(inner: Arc<Inner>, mut rx: mpsc::Receiver<ProviderMessage>) {
    let mut conns: HashMap<u64, [u8; 32]> = HashMap::new();
    let mut reqs: HashMap<(u64, u64), ReqState> = HashMap::new();
    let mut pending: HashMap<u64, ([u8; 32], u64)> = HashMap::new();
    while let Some(msg) = rx.recv().await {
        match msg {
            ProviderMessage::ClientConnected(m) => {
                let contact = match m.endpoint_id {
                    Some(id) => inner.peer_by_endpoint(&id).await.map(|c| c.identity_pk),
                    None => None,
                };
                let verdict = match contact {
                    Some(pk) => {
                        conns.insert(m.connection_id, pk);
                        Ok(())
                    }
                    None => Err(AbortReason::Permission),
                };
                m.tx.send(verdict).await.ok();
            }
            ProviderMessage::GetRequestReceived(m) => {
                let key = (m.connection_id, m.request_id);
                let Some(pk) = conns.get(&m.connection_id).copied() else {
                    m.tx.send(Err(AbortReason::Permission)).await.ok();
                    continue;
                };
                let hash: [u8; 32] = *m.request.hash.as_bytes();
                let origin = inner
                    .store
                    .run(move |c| store::blob_get(c, &hash))
                    .await
                    .ok()
                    .flatten()
                    .map(|b| b.origin);
                let (verdict, counts) = match origin {
                    // Desconhecido: o store responde "não tenho"; um arquivo meu: uso normal.
                    None | Some(BlobOrigin::Own) => (Ok(()), false),
                    // Semear o que não é meu depende do papel e do budget (spec 8.2/8.3).
                    Some(_) if inner.role() < Role::LeafPlus => {
                        (Err(AbortReason::Permission), false)
                    }
                    Some(_) if budget_exceeded(&inner).await => {
                        (Err(AbortReason::RateLimited), false)
                    }
                    Some(_) => (Ok(()), true),
                };
                if verdict.is_ok() {
                    reqs.insert(key, ReqState { pk, counts });
                }
                m.tx.send(verdict).await.ok();
            }
            ProviderMessage::Throttle(m) => {
                let key = (m.connection_id, m.request_id);
                inner.files_sent_bytes.fetch_add(m.size, Ordering::Relaxed);
                if let Some(r) = reqs.get(&key).filter(|r| r.counts) {
                    let e = pending.entry(m.connection_id).or_insert((r.pk, 0));
                    e.1 += m.size;
                }
                // O atraso não pode travar os outros eventos: cada espera roda à parte.
                let limiter = inner.limiter();
                let size = m.size as usize;
                tokio::spawn(async move {
                    limiter.acquire(size).await;
                    m.tx.send(Ok(())).await.ok();
                });
            }
            ProviderMessage::ConnectionClosed(m) => {
                conns.remove(&m.connection_id);
                reqs.retain(|(c, _), _| *c != m.connection_id);
                if let Some((pk, bytes)) = pending.remove(&m.connection_id) {
                    let month = current_month();
                    let _ = inner
                        .store
                        .run(move |c| store::contribution_add(c, month, &pk, bytes, 1))
                        .await;
                }
            }
            _ => {}
        }
    }
}

/// Mantém a agenda de endereços e as tags: recria tags que faltam e aposenta blobs replicados
/// cujo prazo venceu (a coleta de lixo do `iroh-blobs` apaga o que ficou sem tag).
pub(super) async fn maintenance_loop(inner: Arc<Inner>) {
    let mut tick = tokio::time::interval(MAINTENANCE_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        if let Err(e) = maintain(&inner).await {
            tracing::debug!("manutenção de blobs falhou: {e}");
        }
    }
}

pub(super) async fn maintain(inner: &Inner) -> Result<()> {
    let now = unix_now_ms();
    let (rows, expired) = inner
        .store
        .run(move |c| Ok((store::blob_list(c)?, store::blobs_expired(c, now)?)))
        .await?;
    let tags = inner.blobs.store.tags();
    for h in expired {
        let hash = Hash::from_bytes(h);
        tags.delete(tag_name(&hash)).await.map_err(blob_err)?;
        inner.store.run(move |c| store::blob_delete(c, &h)).await?;
    }
    for row in rows
        .into_iter()
        .filter(|r| r.expires_at.is_none_or(|e| e > now))
    {
        let hash = Hash::from_bytes(row.hash);
        if tags.get(tag_name(&hash)).await.map_err(blob_err)?.is_none() {
            tags.set(tag_name(&hash), HashAndFormat::hash_seq(hash))
                .await
                .map_err(blob_err)?;
        }
    }
    Ok(())
}

fn emit_progress(inner: &Inner, hash: [u8; 32], name: &str, done: u64, total: u64, upload: bool) {
    inner.emit(Event::TransferProgress {
        id: hash,
        name: name.to_owned(),
        done,
        total,
        upload,
    });
}

/// Cifra e importa `path` **uma vez** e envia o mesmo `FileRef` a cada contato de `contacts`.
/// Como todos recebem o mesmo blob, quem baixar primeiro vira fonte para os outros.
pub(super) async fn send_file(
    inner: &Inner,
    contacts: &[&str],
    path: &Path,
) -> Result<(Vec<SentMessage>, FileRef)> {
    if contacts.is_empty() {
        return Err(Error::Contact("nenhum destinatário".into()));
    }
    // Valida todos os destinatários antes de gastar tempo cifrando.
    for name in contacts {
        let c = inner.book().resolve(name)?.clone();
        if !c.is_mutual() {
            return Err(Error::Contact(format!(
                "{name}: o contato ainda não aceitou (não é mútuo)"
            )));
        }
    }
    let meta = tokio::fs::metadata(path).await?;
    if !meta.is_file() {
        return Err(Error::Contact(format!(
            "{} não é um arquivo",
            path.display()
        )));
    }
    let max = inner.cfg.max_file_mb.saturating_mul(1024 * 1024);
    if meta.len() > max {
        return Err(Error::Contact(format!(
            "arquivo de {} excede o limite de {} MiB",
            crate::human_size(meta.len()),
            inner.cfg.max_file_mb
        )));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "arquivo".into());
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    let mut tmp = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut tmp);
    let tmp_tag = format!("enx/tmp/{}", data_encoding::HEXLOWER.encode(&tmp));

    let total = meta.len();
    let last = std::sync::atomic::AtomicU64::new(0);
    let imp = blobs::import_file(&inner.blobs.store, path, &key, &tmp_tag, |read| {
        if read - last.load(Ordering::Relaxed) >= progress_step(total) || read == total {
            last.store(read, Ordering::Relaxed);
            emit_progress(inner, [0; 32], &name, read, total, true);
        }
    })
    .await?;
    // Tag definitiva (nome pelo hash) e remoção da temporária.
    let tags = inner.blobs.store.tags();
    tags.set(tag_name(&imp.root), HashAndFormat::hash_seq(imp.root))
        .await
        .map_err(blob_err)?;
    tags.delete(&tmp_tag).await.map_err(blob_err)?;
    let row = BlobRow {
        hash: *imp.root.as_bytes(),
        origin: BlobOrigin::Own,
        plain_size: imp.plain_size,
        enc_size: imp.enc_size,
        name: name.clone(),
        created_at: unix_now_ms(),
        expires_at: None,
    };
    inner
        .store
        .run(move |db| store::blob_upsert(db, &row))
        .await?;

    let fr = FileRef {
        blob_hash: *imp.root.as_bytes(),
        size: imp.plain_size,
        mime: guess_mime(&name).to_owned(),
        name,
        file_key: key,
    };
    let payload = fr.encode()?;
    let mut sent = Vec::new();
    for contact in contacts {
        sent.push(
            super::send_envelope(inner, contact, EnvelopeKind::FileRef, payload.clone()).await?,
        );
    }
    Ok((sent, fr))
}

/// Um arquivo (`FileRef`) na conversa com um contato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Posição (1 = o mais antigo) para usar em `download_file`.
    pub index: usize,
    pub message_id: [u8; 32],
    pub from_me: bool,
    pub name: String,
    pub size: u64,
    pub mime: String,
    /// Já está completo no store local (enviado por mim ou baixado).
    pub available: bool,
}

async fn entries(inner: &Inner, c: &Contact) -> Result<Vec<(FileEntry, FileRef)>> {
    let conv = dm_conversation_id(&inner.identity.identity_pk(), &c.identity_pk);
    let rows = inner
        .store
        .run(move |db| store::filerefs_in_conversation(db, &conv))
        .await?;
    let max = inner.cfg.max_file_mb.saturating_mul(1024 * 1024);
    let mut out = Vec::new();
    for r in rows {
        let Some(plain) = r.plaintext.as_deref() else {
            continue;
        };
        let Ok(fr) = FileRef::decode(plain, max) else {
            continue;
        };
        let root = Hash::from_bytes(fr.blob_hash);
        let available = blobs::is_complete(&inner.blobs.store, root)
            .await
            .unwrap_or(false);
        out.push((
            FileEntry {
                index: out.len() + 1,
                message_id: r.envelope.id,
                from_me: r.outgoing,
                name: sanitize_name(&fr.name),
                size: fr.size,
                mime: fr.mime.clone(),
                available,
            },
            fr,
        ));
    }
    Ok(out)
}

pub(super) async fn list_files(inner: &Inner, contact: &str) -> Result<Vec<FileEntry>> {
    let c = inner.book().resolve(contact)?.clone();
    Ok(entries(inner, &c)
        .await?
        .into_iter()
        .map(|(e, _)| e)
        .collect())
}

/// Resultado de [`download`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub path: PathBuf,
    pub size: u64,
}

/// Contatos mútuos que aceitam uma conexão de blobs agora (descarta os offline para o
/// downloader não perder tempo tentando cada segmento neles).
async fn reachable_providers(inner: &Inner) -> Vec<EndpointId> {
    let timeout = Duration::from_secs(inner.cfg.dial_timeout_secs);
    let peers = inner.peers().await.unwrap_or_default();
    let probes = peers.into_iter().map(|c| {
        let ep = inner.endpoint.clone();
        async move {
            let conn =
                tokio::time::timeout(timeout, ep.connect(c.node_addr.clone(), iroh_blobs::ALPN))
                    .await;
            match conn {
                Ok(Ok(conn)) => {
                    conn.close(0u32.into(), b"probe");
                    Some(c.node_addr.id)
                }
                _ => None,
            }
        }
    });
    futures_join_all(probes)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// Roda os futuros juntos e devolve os resultados na ordem.
pub(super) async fn futures_join_all<F: std::future::Future>(
    futs: impl IntoIterator<Item = F>,
) -> Vec<F::Output> {
    let mut set = n0_future::FuturesOrdered::new();
    for f in futs {
        set.push_back(f);
    }
    let mut out = Vec::new();
    while let Some(x) = n0_future::StreamExt::next(&mut set).await {
        out.push(x);
    }
    out
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (name.to_owned(), String::new()),
    };
    (1..10_000)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Baixa (de todos os contatos que tiverem o blob), decifra para `dest_dir` e passa a semear.
/// `which` é a posição em `files` (1, 2, ...) ou um prefixo hexadecimal do id da mensagem.
pub(super) async fn download(
    inner: &Inner,
    contact: &str,
    which: &str,
    dest_dir: Option<PathBuf>,
) -> Result<Downloaded> {
    let c = inner.book().resolve(contact)?.clone();
    let all = entries(inner, &c).await?;
    let pick = match which.parse::<usize>() {
        Ok(n) if n >= 1 => all.get(n - 1),
        _ => {
            let w = which.to_lowercase();
            let hits: Vec<_> = all
                .iter()
                .filter(|(e, _)| {
                    w.len() >= 4
                        && data_encoding::HEXLOWER
                            .encode(&e.message_id)
                            .starts_with(&w)
                })
                .collect();
            if hits.len() > 1 {
                return Err(Error::Contact(format!("`{which}` é ambíguo")));
            }
            hits.into_iter().next()
        }
    };
    let (entry, fr) = pick
        .cloned()
        .ok_or_else(|| Error::Contact(format!("nenhum arquivo `{which}` na conversa")))?;
    let root = Hash::from_bytes(fr.blob_hash);
    let store = &inner.blobs.store;
    let name = entry.name.clone();

    if !blobs::is_complete(store, root).await? {
        let providers = reachable_providers(inner).await;
        if providers.is_empty() {
            return Err(Error::Blob("nenhum contato online para baixar".into()));
        }
        // Protege o parcial da GC enquanto baixa.
        let _guard = store
            .tags()
            .temp_tag(HashAndFormat::hash_seq(root))
            .await
            .map_err(blob_err)?;
        let total = fr.enc_size();
        let last = std::sync::atomic::AtomicU64::new(0);
        blobs::download(&inner.blobs.downloader, root, providers, None, |done| {
            if done.saturating_sub(last.load(Ordering::Relaxed)) >= PROGRESS_MIN_STEP
                || done >= total
            {
                last.store(done, Ordering::Relaxed);
                emit_progress(inner, fr.blob_hash, &name, done.min(total), total, false);
            }
        })
        .await?;
        if !blobs::is_complete(store, root).await? {
            return Err(Error::Blob("download terminou incompleto".into()));
        }
    }
    // Registra e passa a semear (respeitando papel e budget na hora de servir).
    store
        .tags()
        .set(tag_name(&root), HashAndFormat::hash_seq(root))
        .await
        .map_err(blob_err)?;
    let row = BlobRow {
        hash: fr.blob_hash,
        origin: BlobOrigin::Downloaded,
        plain_size: fr.size,
        enc_size: fr.enc_size(),
        name: fr.name.clone(),
        created_at: unix_now_ms(),
        expires_at: None,
    };
    inner
        .store
        .run(move |db| store::blob_upsert(db, &row))
        .await?;

    let dir = dest_dir
        .or_else(|| inner.cfg.download_dir.clone())
        .unwrap_or_else(|| inner.cfg.data_dir.join("downloads"));
    tokio::fs::create_dir_all(&dir).await?;
    let final_path = unique_path(&dir, &entry.name);
    let part = final_path.with_extension("enxame-part");
    let res = blobs::export_file(store, root, &fr.file_key, fr.size, &part).await;
    if let Err(e) = res {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    tokio::fs::rename(&part, &final_path).await?;
    emit_progress(
        inner,
        fr.blob_hash,
        &name,
        fr.enc_size(),
        fr.enc_size(),
        false,
    );
    Ok(Downloaded {
        path: final_path,
        size: fr.size,
    })
}

/// Pedido para replicar blobs oferecidos por um contato (carrier `Relay`/`SuperPeer`).
pub(super) struct PrefetchJob {
    pub offerer: Contact,
    pub blobs: Vec<BlobOffer>,
}

/// Replica, um a um, os blobs oferecidos. Cada um respeita o tamanho máximo, a quota de carry do
/// papel e o tamanho que o ofertante declarou.
pub(super) async fn prefetch_worker(
    inner: Arc<Inner>,
    mut rx: mpsc::UnboundedReceiver<PrefetchJob>,
) {
    while let Some(job) = rx.recv().await {
        for b in job.blobs {
            if let Err(e) = prefetch_one(&inner, &job.offerer, &b).await {
                tracing::debug!(offerer = %job.offerer.fingerprint(), "pré-replicação ignorada: {e}");
            }
        }
    }
}

async fn prefetch_one(inner: &Inner, offerer: &Contact, b: &BlobOffer) -> Result<()> {
    let limits = CarryLimits::for_role(inner.role(), &inner.cfg.budget);
    if !limits.accepts_files {
        return Ok(());
    }
    if b.enc_size > inner.cfg.prefetch_max_mb.saturating_mul(1024 * 1024) {
        return Err(Error::Blob(format!(
            "blob de {} bytes acima de prefetch_max_mb",
            b.enc_size
        )));
    }
    let now = unix_now_ms();
    let used = inner
        .store
        .run(move |c| store::blobs_prefetched_bytes(c, now))
        .await?;
    if used.saturating_add(b.enc_size) > limits.total_bytes {
        return Err(Error::Blob("sem espaço na quota de carry".into()));
    }
    let root = Hash::from_bytes(b.hash);
    let store = &inner.blobs.store;
    if !blobs::is_complete(store, root).await? {
        let _guard = store
            .tags()
            .temp_tag(HashAndFormat::hash_seq(root))
            .await
            .map_err(blob_err)?;
        blobs::download(
            &inner.blobs.downloader,
            root,
            vec![offerer.node_addr.id],
            Some(b.enc_size),
            |_| {},
        )
        .await?;
        if !blobs::is_complete(store, root).await? {
            return Err(Error::Blob("pré-replicação incompleta".into()));
        }
    }
    store
        .tags()
        .set(tag_name(&root), HashAndFormat::hash_seq(root))
        .await
        .map_err(blob_err)?;
    let row = BlobRow {
        hash: b.hash,
        origin: BlobOrigin::Prefetched,
        plain_size: 0,
        enc_size: b.enc_size,
        name: "(replicado)".into(),
        created_at: now,
        expires_at: Some(now.saturating_add(limits.ttl.as_millis() as u64)),
    };
    inner
        .store
        .run(move |c| store::blob_upsert(c, &row))
        .await?;
    Ok(())
}

/// Blob que este nó guarda como carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefetchedBlob {
    pub hash: [u8; 32],
    pub enc_size: u64,
    /// Quando a cópia expira (ms desde a época Unix).
    pub expires_at: u64,
}

pub(super) async fn list_prefetched(inner: &Inner) -> Result<Vec<PrefetchedBlob>> {
    let rows = inner.store.run(|c| store::blob_list(c)).await?;
    Ok(rows
        .into_iter()
        .filter(|r| r.origin == BlobOrigin::Prefetched)
        .map(|r| PrefetchedBlob {
            hash: r.hash,
            enc_size: r.enc_size,
            expires_at: r.expires_at.unwrap_or(u64::MAX),
        })
        .collect())
}

pub(super) async fn has_blob(inner: &Inner, hash: [u8; 32]) -> bool {
    blobs::is_complete(&inner.blobs.store, Hash::from_bytes(hash))
        .await
        .unwrap_or(false)
}

/// Faz vencer agora as cópias replicadas e roda a manutenção (para testes).
pub(super) async fn expire_prefetched_now(inner: &Inner) -> Result<()> {
    let now = unix_now_ms();
    inner
        .store
        .run(move |c| {
            c.execute(
                "UPDATE blobs SET expires_at = ?1 WHERE origin = 2",
                [now as i64],
            )
            .map_err(|e| Error::Store(e.to_string()))?;
            Ok(())
        })
        .await?;
    maintain(inner).await
}
