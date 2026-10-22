//! Endpoint iroh, discovery (DNS/pkarr + mDNS) e métricas de caminho (spec 11).

use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use iroh::tls::CaTlsConfig;
use iroh::{
    endpoint::{presets, Connection},
    unstable_net_report::NetReport,
    Endpoint, RelayMode, Watcher,
};
use iroh_mdns_address_lookup::MdnsAddressLookup;
use n0_future::StreamExt;
use rustls_pki_types::CertificateDer;
use serde::{Deserialize, Serialize};
use tokio::{sync::watch, task::JoinHandle};

use crate::{
    config::{Config, RelaySetting},
    identity::Identity,
    proto::{ALPN_CONTACT, ALPN_SYNC},
    Error, Result,
};

/// Cria o endpoint com a chave de dispositivo da identidade, os ALPNs do Enxame e o relay
/// configurado. Com `mdns`, registra também a descoberta em rede local.
pub async fn bind_endpoint(cfg: &Config, identity: &Identity) -> Result<Endpoint> {
    let alpns = vec![ALPN_CONTACT.to_vec(), ALPN_SYNC.to_vec()];
    let base = if cfg.dns_discovery {
        Endpoint::builder(presets::N0)
    } else {
        Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Default)
    };
    let builder = match &cfg.relay {
        RelaySetting::Default => base,
        RelaySetting::Disabled => base.relay_mode(RelayMode::Disabled),
        RelaySetting::Custom(url) => {
            let urls = url
                .split(',')
                .map(|u| {
                    u.trim()
                        .parse::<iroh::RelayUrl>()
                        .map_err(|e| Error::Config(format!("relay `{u}`: {e}")))
                })
                .collect::<Result<Vec<_>>>()?;
            base.relay_mode(RelayMode::custom(urls))
        }
    };
    let dns = cfg.dns_server_addrs()?;
    let builder = if dns.is_empty() {
        builder
    } else {
        let servers = dns.into_iter().map(iroh::dns::NameserverConfig::udp);
        builder.dns_resolver(
            iroh::dns::DnsResolver::builder()
                .add_nameserver_configs(servers)
                .build(),
        )
    };
    let builder = match &cfg.relay_ca_file {
        Some(path) => {
            let roots = load_pem_certs(path)?;
            builder.ca_tls_config(CaTlsConfig::custom_roots(roots))
        }
        None => builder,
    };
    let endpoint = builder
        .secret_key(identity.device_secret().clone())
        .alpns(alpns)
        .bind()
        .await
        .map_err(|e| Error::Net(format!("bind: {e}")))?;

    if cfg.mdns {
        match MdnsAddressLookup::builder().build(endpoint.id()) {
            Ok(mdns) => match endpoint.address_lookup() {
                Ok(services) => services.add(mdns),
                Err(e) => tracing::warn!("mDNS não registrado: {e}"),
            },
            Err(e) => tracing::warn!("mDNS indisponível: {e}"),
        }
    }
    Ok(endpoint)
}

/// Lê certificados PEM (`-----BEGIN CERTIFICATE-----`) como DER.
fn load_pem_certs(path: &std::path::Path) -> Result<Vec<CertificateDer<'static>>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Config(format!("relay_ca_file {}: {e}", path.display())))?;
    let mut out = Vec::new();
    let mut body = String::new();
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with("-----BEGIN CERTIFICATE") {
            inside = true;
            body.clear();
        } else if line.starts_with("-----END CERTIFICATE") {
            let der = data_encoding::BASE64
                .decode(body.as_bytes())
                .map_err(|e| Error::Config(format!("relay_ca_file: PEM inválido: {e}")))?;
            out.push(CertificateDer::from(der));
            inside = false;
        } else if inside {
            body.push_str(line);
        }
    }
    if out.is_empty() {
        return Err(Error::Config(format!(
            "relay_ca_file {}: nenhum certificado PEM",
            path.display()
        )));
    }
    Ok(out)
}

/// Tipo de NAT, como na spec 8.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NatKind {
    /// Endereço público igual a um endereço local: sem NAT.
    Open,
    /// Mapeamento independente do destino: hole punching costuma funcionar.
    Cone,
    /// Mapeamento varia por destino: tende a cair em relay.
    Symmetric,
    Unknown,
}

/// Resumo da sondagem de rede do iroh (QAD) traduzido para os termos do Enxame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NatReport {
    pub kind: NatKind,
    pub udp: bool,
    pub global_v4: Option<SocketAddr>,
    pub mapping_varies_by_dest: Option<bool>,
    pub preferred_relay: Option<String>,
}

impl NatReport {
    fn unknown() -> Self {
        Self {
            kind: NatKind::Unknown,
            udp: false,
            global_v4: None,
            mapping_varies_by_dest: None,
            preferred_relay: None,
        }
    }
}

/// Classifica o NAT a partir do relatório do iroh e dos endereços locais do endpoint.
pub fn classify_nat(report: &NetReport, local_ips: &[std::net::IpAddr]) -> NatReport {
    let global_v4 = report.global_v4.map(SocketAddr::V4);
    let varies = report.mapping_varies_by_dest();
    let kind = if !report.has_udp() {
        NatKind::Unknown
    } else {
        match varies {
            Some(true) => NatKind::Symmetric,
            Some(false) => match global_v4 {
                Some(g) if local_ips.contains(&g.ip()) => NatKind::Open,
                Some(_) => NatKind::Cone,
                None => NatKind::Unknown,
            },
            None => NatKind::Unknown,
        }
    };
    NatReport {
        kind,
        udp: report.has_udp(),
        global_v4,
        mapping_varies_by_dest: varies,
        preferred_relay: report.preferred_relay.as_ref().map(|u| u.to_string()),
    }
}

/// Aguarda o primeiro relatório de rede (até `timeout`) e o classifica.
pub async fn nat_report(endpoint: &Endpoint, timeout: Duration) -> NatReport {
    match tokio::time::timeout(timeout, endpoint.net_report().initialized()).await {
        Ok(report) => {
            let local: Vec<_> = endpoint
                .addr()
                .ip_addrs()
                .map(|a| a.ip())
                .chain(endpoint.bound_sockets().iter().map(|a| a.ip()))
                .collect();
            classify_nat(&report, &local)
        }
        Err(_) => NatReport::unknown(),
    }
}

/// Tipo de caminho de uma conexão (spec 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PathKind {
    /// Ainda sem caminho selecionado.
    #[default]
    None,
    /// Caminho IP direto selecionado.
    Direct,
    /// Via relay, sem caminho direto aberto.
    Relay,
    /// Via relay enquanto um caminho direto já está aberto (hole punching em curso).
    Mixed,
}

/// Métricas por conexão: principal entregável de validação técnica da Fase 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathMetrics {
    pub kind: PathKind,
    pub selected_addr: Option<String>,
    pub rtt: Option<Duration>,
    /// Tempo desde o início do monitor até o primeiro caminho direto selecionado.
    pub time_to_direct: Option<Duration>,
    /// Trocas entre direto e relay.
    pub path_switches: u32,
    pub open_paths: usize,
}

/// Observa os caminhos de uma conexão e publica [`PathMetrics`].
#[derive(Debug)]
pub struct PathMonitor {
    rx: watch::Receiver<PathMetrics>,
    task: JoinHandle<()>,
}

impl PathMonitor {
    /// Começa a observar `conn`. `started` é o instante em que a conexão começou a ser
    /// discada: dele parte o `time_to_direct`.
    pub fn spawn(conn: Connection, started: Instant) -> Self {
        let (tx, rx) = watch::channel(PathMetrics::default());
        let task = tokio::spawn(async move {
            let mut stream = conn.paths_stream();
            let mut last_is_ip: Option<bool> = None;
            while let Some(list) = stream.next().await {
                let selected = list.iter().find(|p| p.is_selected());
                let any_ip = list.iter().any(|p| p.is_ip());
                let (kind, addr, rtt) = match &selected {
                    Some(p) if p.is_ip() => (
                        PathKind::Direct,
                        Some(format!("{:?}", p.remote_addr())),
                        Some(p.rtt()),
                    ),
                    Some(p) if any_ip => (
                        PathKind::Mixed,
                        Some(format!("{:?}", p.remote_addr())),
                        Some(p.rtt()),
                    ),
                    Some(p) => (
                        PathKind::Relay,
                        Some(format!("{:?}", p.remote_addr())),
                        Some(p.rtt()),
                    ),
                    None => (PathKind::None, None, None),
                };
                let is_ip = selected.as_ref().map(|p| p.is_ip());
                let open_paths = list.len();
                tx.send_modify(|m| {
                    if let (Some(prev), Some(now)) = (last_is_ip, is_ip) {
                        if prev != now {
                            m.path_switches += 1;
                        }
                    }
                    if kind == PathKind::Direct && m.time_to_direct.is_none() {
                        m.time_to_direct = Some(started.elapsed());
                    }
                    m.kind = kind;
                    m.selected_addr = addr;
                    m.rtt = rtt;
                    m.open_paths = open_paths;
                });
                if is_ip.is_some() {
                    last_is_ip = is_ip;
                }
            }
        });
        Self { rx, task }
    }

    /// Métricas atuais.
    pub fn snapshot(&self) -> PathMetrics {
        self.rx.borrow().clone()
    }

    /// Espera até um caminho direto ser selecionado, ou `timeout`. Devolve o último snapshot.
    pub async fn wait_direct(&mut self, timeout: Duration) -> PathMetrics {
        let _ =
            tokio::time::timeout(timeout, self.rx.wait_for(|m| m.kind == PathKind::Direct)).await;
        self.snapshot()
    }
}

impl Drop for PathMonitor {
    fn drop(&mut self) {
        self.task.abort();
    }
}
