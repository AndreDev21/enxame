use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Qual relay usar (spec 11).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelaySetting {
    /// Relays públicos padrão do iroh (apenas desenvolvimento).
    #[default]
    Default,
    /// Sem relay: só conexões diretas (testes, LAN).
    Disabled,
    /// Relay(s) próprio(s) (produção, ver `deploy/`): uma URL ou várias separadas por vírgula.
    /// Com ao menos dois relays o `diag` consegue distinguir NAT cone de simétrico.
    Custom(String),
}

impl RelaySetting {
    /// Interpreta `default`, `disabled` ou uma URL (`ENXAME_RELAY`, `--relay`).
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim() {
            "" | "default" => Ok(Self::Default),
            "disabled" | "off" | "none" => Ok(Self::Disabled),
            urls => {
                for url in urls.split(',') {
                    url.trim()
                        .parse::<iroh::RelayUrl>()
                        .map_err(|e| Error::Config(format!("relay `{url}`: {e}")))?;
                }
                Ok(Self::Custom(urls.to_owned()))
            }
        }
    }
}

/// Configuração do nó. Arquivo TOML em `<data_dir>/config.toml` + override por `ENXAME_*`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Diretório de dados (chaves em arquivo, contatos). Não é lido do TOML.
    #[serde(skip)]
    pub data_dir: PathBuf,
    /// Nome exibido nos tickets.
    pub display_name: String,
    /// Relay a usar.
    pub relay: RelaySetting,
    /// CA privada (PEM) em que confiar para o relay, para relays self-hosted com certificado
    /// próprio. Substitui as raízes públicas: só vale com `relay = "<url>"`.
    pub relay_ca_file: Option<PathBuf>,
    /// Descoberta mDNS na rede local.
    pub mdns: bool,
    /// Discovery por DNS/pkarr (servidores do n0 por padrão). Desligue em redes isoladas.
    pub dns_discovery: bool,
    /// Tempo máximo esperando a decisão do outro lado ao adicionar contato.
    pub contact_request_timeout_secs: u64,
    /// Tempo máximo para discar um contato ao sincronizar.
    pub dial_timeout_secs: u64,
    /// Intervalo da sincronização automática com os contatos, em segundos (0 desliga).
    pub sync_interval_secs: u64,
    /// Validade padrão de um ticket.
    pub ticket_ttl_secs: u64,
    /// Tamanho máximo de frame de protocolo, em bytes.
    pub max_frame_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            display_name: "enxame".to_owned(),
            relay: RelaySetting::default(),
            relay_ca_file: None,
            mdns: true,
            dns_discovery: true,
            contact_request_timeout_secs: 300,
            dial_timeout_secs: 15,
            sync_interval_secs: 30,
            ticket_ttl_secs: 24 * 3600,
            max_frame_bytes: 1024 * 1024,
        }
    }
}

impl Config {
    /// Carrega `<data_dir>/config.toml` (se existir) e aplica overrides de ambiente.
    pub fn load(data_dir: Option<PathBuf>) -> Result<Self> {
        let data_dir = data_dir
            .or_else(|| std::env::var_os("ENXAME_DATA_DIR").map(PathBuf::from))
            .unwrap_or_else(default_data_dir);
        let path = data_dir.join("config.toml");
        let mut cfg = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<Config>(&text)
                .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e.into()),
        };
        cfg.data_dir = data_dir;
        if let Ok(v) = std::env::var("ENXAME_DISPLAY_NAME") {
            cfg.display_name = v;
        }
        if let Ok(v) = std::env::var("ENXAME_RELAY") {
            cfg.relay = RelaySetting::parse(&v)?;
        }
        if let Some(v) = std::env::var_os("ENXAME_RELAY_CA_FILE") {
            cfg.relay_ca_file = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("ENXAME_MDNS") {
            cfg.mdns = !matches!(v.as_str(), "0" | "false" | "off");
        }
        if let Ok(v) = std::env::var("ENXAME_DNS_DISCOVERY") {
            cfg.dns_discovery = !matches!(v.as_str(), "0" | "false" | "off");
        }
        Ok(cfg)
    }

    /// Configuração para testes: diretório dado, sem relay e sem mDNS.
    pub fn for_tests(data_dir: PathBuf, display_name: &str) -> Self {
        Self {
            data_dir,
            display_name: display_name.to_owned(),
            relay: RelaySetting::Disabled,
            mdns: false,
            dns_discovery: false,
            dial_timeout_secs: 3,
            sync_interval_secs: 0,
            ..Self::default()
        }
    }
}

/// Diretório de dados padrão por SO (XDG no Linux, `%APPDATA%` no Windows).
pub fn default_data_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("enxame")
}
