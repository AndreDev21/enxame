//! Arquivos sobre `iroh-blobs` (spec 10, ADR 0007).
//!
//! O arquivo é cifrado em streaming ([`crate::filecrypt`]) e o **ciphertext** é cortado em
//! segmentos de 8 MiB; cada segmento vira um blob e a lista de hashes vira um `HashSeq` cuja raiz
//! é o `blob_hash` do `FileRef`. Isso dá, sem código de rede nosso: verificação BLAKE3 por
//! segmento, retomada por segmento e **download simultâneo de várias fontes** (o `Downloader` do
//! `iroh-blobs` reparte os segmentos entre os provedores).

use std::path::Path;

use bytes::Bytes;
use iroh::EndpointId;
use iroh_blobs::{
    api::{
        blobs::AddBytesOptions,
        downloader::{DownloadOptions, DownloadProgressItem, Downloader, SplitStrategy},
        Store,
    },
    hashseq::HashSeq,
    BlobFormat, Hash, HashAndFormat,
};
use n0_future::StreamExt;
use tokio::io::AsyncReadExt;

use crate::{
    filecrypt::{encrypted_len, Decryptor, Encryptor},
    Error, Result,
};

/// Tamanho de cada segmento de ciphertext.
pub const SEGMENT_SIZE: usize = 8 * 1024 * 1024;
const READ_BUF: usize = 1024 * 1024;

fn blob_err(e: impl std::fmt::Display) -> Error {
    Error::Blob(e.to_string())
}

/// Resultado de importar um arquivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    /// Raiz (`HashSeq`) a colocar no `FileRef`.
    pub root: Hash,
    /// Tamanho em claro.
    pub plain_size: u64,
    /// Tamanho do ciphertext (soma dos segmentos).
    pub enc_size: u64,
    pub segments: usize,
}

/// Cifra `path` com `key` e importa o ciphertext no store, protegido por uma tag de nome `tag`.
/// `progress` recebe o total de bytes em claro lidos até o momento.
pub async fn import_file(
    store: &Store,
    path: &Path,
    key: &[u8; 32],
    tag: &str,
    mut progress: impl FnMut(u64) + Send,
) -> Result<Imported> {
    let mut file = tokio::fs::File::open(path).await?;
    let plain_size = file.metadata().await?.len();
    let mut enc = Encryptor::new(key)?;
    let mut buf = vec![0u8; READ_BUF];
    let mut pending: Vec<u8> = Vec::with_capacity(SEGMENT_SIZE + READ_BUF + 1024);
    let mut hashes: Vec<Hash> = Vec::new();
    // As tags temporárias protegem os segmentos da GC até a raiz ganhar a tag nomeada.
    let mut guards = Vec::new();
    let mut read_total = 0u64;
    let mut enc_size = 0u64;

    let mut flush = |seg: Vec<u8>| {
        enc_size += seg.len() as u64;
        let store = store.clone();
        async move {
            let t = store
                .blobs()
                .add_bytes(Bytes::from(seg))
                .temp_tag()
                .await
                .map_err(blob_err)?;
            Ok::<_, Error>(t)
        }
    };
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        read_total += n as u64;
        enc.push(&buf[..n], &mut pending)?;
        while pending.len() >= SEGMENT_SIZE {
            let rest = pending.split_off(SEGMENT_SIZE);
            let seg = std::mem::replace(&mut pending, rest);
            let t = flush(seg).await?;
            hashes.push(t.hash());
            guards.push(t);
        }
        progress(read_total);
    }
    enc.finish(&mut pending)?;
    while pending.len() > SEGMENT_SIZE {
        let rest = pending.split_off(SEGMENT_SIZE);
        let seg = std::mem::replace(&mut pending, rest);
        let t = flush(seg).await?;
        hashes.push(t.hash());
        guards.push(t);
    }
    if !pending.is_empty() {
        let t = flush(std::mem::take(&mut pending)).await?;
        hashes.push(t.hash());
        guards.push(t);
    }
    if enc_size != encrypted_len(plain_size) {
        return Err(Error::Blob(format!(
            "tamanho do ciphertext {enc_size} difere do previsto {}",
            encrypted_len(plain_size)
        )));
    }
    let seq: HashSeq = hashes.iter().copied().collect();
    let root = store
        .blobs()
        .add_bytes_with_opts(AddBytesOptions {
            data: seq.into_inner(),
            format: BlobFormat::HashSeq,
        })
        .with_named_tag(tag)
        .await
        .map_err(blob_err)?
        .hash;
    drop(guards);
    Ok(Imported {
        root,
        plain_size,
        enc_size,
        segments: hashes.len(),
    })
}

/// A raiz e todos os segmentos estão completos no store local?
pub async fn is_complete(store: &Store, root: Hash) -> Result<bool> {
    let status = store.blobs().status(root).await.map_err(blob_err)?;
    if !matches!(status, iroh_blobs::api::blobs::BlobStatus::Complete { .. }) {
        return Ok(false);
    }
    let seq = HashSeq::try_from(store.blobs().get_bytes(root).await.map_err(blob_err)?)
        .map_err(|e| Error::Blob(format!("raiz inválida: {e}")))?;
    for h in seq.iter() {
        let st = store.blobs().status(h).await.map_err(blob_err)?;
        if !matches!(st, iroh_blobs::api::blobs::BlobStatus::Complete { .. }) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Escolhe provedores em rodízio: cada segmento começa por um provedor diferente, o que reparte a
/// carga entre as fontes de forma previsível (o `Shuffled` do iroh sorteia).
#[derive(Debug, Clone)]
pub struct RoundRobin {
    nodes: Vec<EndpointId>,
    next: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl RoundRobin {
    pub fn new(nodes: Vec<EndpointId>) -> Self {
        Self {
            nodes,
            next: Default::default(),
        }
    }
}

impl iroh_blobs::api::downloader::ContentDiscovery for RoundRobin {
    fn find_providers(&self, _: HashAndFormat) -> n0_future::stream::Boxed<EndpointId> {
        let n = self.nodes.len().max(1);
        let start = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % n;
        let mut nodes = self.nodes.clone();
        nodes.rotate_left(start);
        n0_future::stream::iter(nodes).boxed()
    }
}

/// Baixa a raiz de qualquer um de `providers`, repartindo os segmentos entre eles. `progress`
/// recebe o total de bytes já presentes. A retomada é automática: o store guarda o parcial.
pub async fn download(
    downloader: &Downloader,
    root: Hash,
    providers: Vec<EndpointId>,
    max_bytes: Option<u64>,
    mut progress: impl FnMut(u64) + Send,
) -> Result<()> {
    if providers.is_empty() {
        return Err(Error::Blob("nenhum contato para baixar".into()));
    }
    let opts = DownloadOptions::new(
        HashAndFormat::hash_seq(root),
        RoundRobin::new(providers),
        SplitStrategy::Split,
    );
    let mut stream = downloader
        .download_with_opts(opts)
        .stream()
        .await
        .map_err(blob_err)?;
    let mut failed = false;
    while let Some(item) = stream.next().await {
        match item {
            DownloadProgressItem::Progress(total) => {
                // Quem oferece declarou um tamanho; passar dele cancela (cota do carrier).
                if max_bytes.is_some_and(|m| total > m) {
                    return Err(Error::Blob("o blob é maior do que o anunciado".into()));
                }
                progress(total)
            }
            DownloadProgressItem::Error(e) => return Err(blob_err(e)),
            DownloadProgressItem::DownloadError => failed = true,
            _ => {}
        }
    }
    if failed {
        return Err(Error::Blob(
            "download incompleto: nenhum contato tinha todos os segmentos".into(),
        ));
    }
    Ok(())
}

/// Decifra o arquivo (a raiz precisa estar completa) para `dest`, conferindo o tamanho final.
pub async fn export_file(
    store: &Store,
    root: Hash,
    key: &[u8; 32],
    plain_size: u64,
    dest: &Path,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let seq = HashSeq::try_from(store.blobs().get_bytes(root).await.map_err(blob_err)?)
        .map_err(|e| Error::Blob(format!("raiz inválida: {e}")))?;
    let mut out = tokio::fs::File::create(dest).await?;
    let mut dec = Decryptor::new(key)?;
    let mut plain = Vec::new();
    let mut written = 0u64;
    for h in seq.iter() {
        let seg = store.blobs().get_bytes(h).await.map_err(blob_err)?;
        dec.push(&seg, &mut plain)?;
        written += plain.len() as u64;
        out.write_all(&plain).await?;
        plain.clear();
    }
    dec.finish(&mut plain)?;
    written += plain.len() as u64;
    out.write_all(&plain).await?;
    out.flush().await?;
    out.sync_all().await?;
    if written != plain_size {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(Error::Blob(format!(
            "tamanho decifrado {written} difere do anunciado {plain_size}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };

    use iroh::{
        address_lookup::MemoryLookup, endpoint::presets, protocol::Router, Endpoint, RelayMode,
    };
    use iroh_blobs::{
        provider::events::{EventMask, EventSender, ProviderMessage, ThrottleMode},
        store::fs::FsStore,
        BlobsProtocol,
    };
    use tempfile::TempDir;

    const KEY: [u8; 32] = [9; 32];

    struct Peer {
        endpoint: Endpoint,
        store: FsStore,
        lookup: MemoryLookup,
        downloader: Downloader,
        /// Bytes de blobs que este par serviu (soma dos eventos de throttle).
        served: Arc<AtomicU64>,
        dir: TempDir,
        _router: Router,
    }

    async fn peer() -> Peer {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::load(dir.path().join("blobs")).await.unwrap();
        let served = Arc::new(AtomicU64::new(0));
        let mask = EventMask {
            throttle: ThrottleMode::Intercept,
            ..EventMask::DEFAULT
        };
        let (events, mut rx) = EventSender::channel(32, mask);
        let counter = served.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let ProviderMessage::Throttle(m) = msg {
                    counter.fetch_add(m.size, Ordering::Relaxed);
                    m.tx.send(Ok(())).await.ok();
                }
            }
        });
        let endpoint = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .bind()
            .await
            .unwrap();
        let lookup = MemoryLookup::new();
        endpoint.address_lookup().unwrap().add(lookup.clone());
        let proto = BlobsProtocol::new(&store, Some(events));
        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, proto)
            .spawn();
        let downloader = Downloader::new(&store, &endpoint);
        Peer {
            endpoint,
            store,
            lookup,
            downloader,
            served,
            dir,
            _router: router,
        }
    }

    /// Cada par aprende o endereço de todos os outros.
    fn introduce(peers: &[&Peer]) {
        for a in peers {
            for b in peers {
                if a.endpoint.id() != b.endpoint.id() {
                    a.lookup.add_endpoint_info(b.endpoint.addr());
                }
            }
        }
    }

    fn write_file(dir: &Path, name: &str, len: usize) -> std::path::PathBuf {
        let p = dir.join(name);
        let data: Vec<u8> = (0..len).map(|i| ((i * 131 + i / 7) % 251) as u8).collect();
        std::fs::write(&p, data).unwrap();
        p
    }

    async fn fetch(p: &Peer, imp: &Imported, from: &[&Peer]) {
        let ids = from.iter().map(|x| x.endpoint.id()).collect();
        download(&p.downloader, imp.root, ids, None, |_| {})
            .await
            .unwrap();
        assert!(is_complete(&p.store, imp.root).await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn import_export_roundtrip_across_segment_boundaries() {
        let a = peer().await;
        for len in [
            0usize,
            1,
            100_000,
            SEGMENT_SIZE - 28 - 16,
            SEGMENT_SIZE,
            2 * SEGMENT_SIZE + 12_345,
        ] {
            let path = write_file(a.dir.path(), &format!("f{len}"), len);
            let imp = import_file(&a.store, &path, &KEY, &format!("t{len}"), |_| {})
                .await
                .unwrap();
            assert_eq!(imp.plain_size, len as u64);
            assert_eq!(imp.enc_size, encrypted_len(len as u64));
            assert!(is_complete(&a.store, imp.root).await.unwrap());
            let out = a.dir.path().join(format!("out{len}"));
            export_file(&a.store, imp.root, &KEY, imp.plain_size, &out)
                .await
                .unwrap();
            assert_eq!(
                std::fs::read(&out).unwrap(),
                std::fs::read(&path).unwrap(),
                "len {len}"
            );
        }
        // Chave errada ou tamanho anunciado errado não produzem arquivo.
        let path = write_file(a.dir.path(), "x", 10);
        let imp = import_file(&a.store, &path, &KEY, "tx", |_| {})
            .await
            .unwrap();
        let bad = a.dir.path().join("bad");
        assert!(export_file(&a.store, imp.root, &[1; 32], 10, &bad)
            .await
            .is_err());
        assert!(export_file(&a.store, imp.root, &KEY, 11, &bad)
            .await
            .is_err());
        assert!(!bad.exists(), "arquivo parcial removido");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_node_downloads_and_decrypts() {
        let (a, b) = (peer().await, peer().await);
        introduce(&[&a, &b]);
        let path = write_file(a.dir.path(), "doc", 3 * SEGMENT_SIZE + 77);
        let imp = import_file(&a.store, &path, &KEY, "doc", |_| {})
            .await
            .unwrap();
        assert!(!is_complete(&b.store, imp.root).await.unwrap());
        fetch(&b, &imp, &[&a]).await;
        let out = b.dir.path().join("doc.out");
        export_file(&b.store, imp.root, &KEY, imp.plain_size, &out)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&path).unwrap());
        assert!(a.served.load(Ordering::Relaxed) >= imp.enc_size);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn two_sources_serve_the_same_download_at_the_same_time() {
        let (a, b, c) = (peer().await, peer().await, peer().await);
        introduce(&[&a, &b, &c]);
        let path = write_file(a.dir.path(), "grande", 10 * SEGMENT_SIZE);
        let imp = import_file(&a.store, &path, &KEY, "g", |_| {})
            .await
            .unwrap();
        fetch(&b, &imp, &[&a]).await; // B vira segunda fonte
        let (a_before, b_before) = (
            a.served.load(Ordering::Relaxed),
            b.served.load(Ordering::Relaxed),
        );
        fetch(&c, &imp, &[&a, &b]).await; // C baixa das duas
        let (from_a, from_b) = (
            a.served.load(Ordering::Relaxed) - a_before,
            b.served.load(Ordering::Relaxed) - b_before,
        );
        assert!(
            from_a > imp.enc_size / 4,
            "A serviu só {from_a} de {}",
            imp.enc_size
        );
        assert!(
            from_b > imp.enc_size / 4,
            "B serviu só {from_b} de {}",
            imp.enc_size
        );
        let out = c.dir.path().join("out");
        export_file(&c.store, imp.root, &KEY, imp.plain_size, &out)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&path).unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_interrupted_download_resumes_without_refetching_what_arrived() {
        let (a, c) = (peer().await, peer().await);
        introduce(&[&a, &c]);
        let path = write_file(a.dir.path(), "retomar", 12 * SEGMENT_SIZE);
        let imp = import_file(&a.store, &path, &KEY, "r", |_| {})
            .await
            .unwrap();
        let before = a.served.load(Ordering::Relaxed);

        // Primeira tentativa: cancela quando passar de ~35 %.
        let opts = DownloadOptions::new(
            HashAndFormat::hash_seq(imp.root),
            RoundRobin::new(vec![a.endpoint.id()]),
            SplitStrategy::Split,
        );
        let mut stream = c
            .downloader
            .download_with_opts(opts)
            .stream()
            .await
            .unwrap();
        while let Some(item) = stream.next().await {
            if let DownloadProgressItem::Progress(t) = item {
                if t > imp.enc_size * 35 / 100 {
                    break;
                }
            }
        }
        drop(stream);
        c.downloader.wait_idle().await.ok();
        let first_run = a.served.load(Ordering::Relaxed) - before;
        assert!(
            !is_complete(&c.store, imp.root).await.unwrap(),
            "ainda incompleto depois do corte"
        );

        // Segunda tentativa: termina, e só pede o que faltava.
        fetch(&c, &imp, &[&a]).await;
        let second_run = a.served.load(Ordering::Relaxed) - before - first_run;
        assert!(
            second_run < imp.enc_size * 85 / 100,
            "a retomada reenviou demais: {second_run} de {} (primeira {first_run})",
            imp.enc_size
        );
        let out = c.dir.path().join("out");
        export_file(&c.store, imp.root, &KEY, imp.plain_size, &out)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&path).unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn download_without_providers_or_with_dead_ones_fails_cleanly() {
        let (a, c) = (peer().await, peer().await);
        introduce(&[&a, &c]);
        let path = write_file(a.dir.path(), "x", 1000);
        let imp = import_file(&a.store, &path, &KEY, "x", |_| {})
            .await
            .unwrap();
        assert!(download(&c.downloader, imp.root, vec![], None, |_| {})
            .await
            .is_err());
        // Um provedor que não tem o arquivo (c não tem; baixa de si mesmo é impossível).
        let d = peer().await;
        introduce(&[&c, &d]);
        let r = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            download(&c.downloader, imp.root, vec![d.endpoint.id()], None, |_| {}),
        )
        .await
        .unwrap();
        assert!(r.is_err());
    }
}
