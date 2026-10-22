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

/// Limites de contribuição do usuário (spec 8.3). Sempre aplicados acima do papel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Budget {
    /// Teto de banda servida a terceiros, em kbps. `None` = 30 % da estimativa passiva de upload
    /// (sem estimativa ainda, sem teto).
    pub max_contrib_kbps: Option<u64>,
    /// Teto mensal de dados servidos a terceiros, em GB (padrão 20 no desktop).
    pub max_contrib_gb_month: u64,
    /// Quota de carry por contato, em MB. `None` = padrão do papel.
    pub carry_quota_mb_per_contact: Option<u64>,
    /// Quota total de carry, em MB. `None` = padrão do papel.
    pub carry_quota_mb_total: Option<u64>,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_contrib_kbps: None,
            max_contrib_gb_month: 20,
            carry_quota_mb_per_contact: None,
            carry_quota_mb_total: None,
        }
    }
}

/// Como o papel é escolhido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoleMode {
    /// Automático, por sinais e histerese (padrão).
    #[default]
    Auto,
    /// Fixo (nó headless: `super_peer`).
    Fixed(crate::Role),
}

impl RoleMode {
    /// Interpreta `auto`, `leaf`, `leaf_plus`, `peer`, `relay` ou `super_peer`.
    pub fn parse(s: &str) -> Result<Self> {
        use crate::Role::*;
        Ok(match s.trim().to_lowercase().replace('-', "_").as_str() {
            "" | "auto" => Self::Auto,
            "leaf" => Self::Fixed(Leaf),
            "leaf_plus" | "leafplus" => Self::Fixed(LeafPlus),
            "peer" => Self::Fixed(Peer),
            "relay" => Self::Fixed(Relay),
            "super_peer" | "superpeer" => Self::Fixed(SuperPeer),
            other => return Err(Error::Config(format!("papel `{other}` desconhecido"))),
        })
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
    /// Servidores DNS (IPs) que o iroh usa no lugar dos do sistema. Útil quando o resolvedor local
    /// descarta consultas com EDNS (alguns roteadores e o MagicDNS do Tailscale), o que impede
    /// alcançar o relay. Ex.: `["1.1.1.1", "8.8.8.8"]`. Vazio = os do sistema.
    pub dns_servers: Vec<String>,
    /// Discovery por DNS/pkarr (servidores do n0 por padrão). Desligue em redes isoladas.
    pub dns_discovery: bool,
    /// Tempo máximo esperando a decisão do outro lado ao adicionar contato.
    pub contact_request_timeout_secs: u64,
    /// Tempo máximo para discar um contato ao sincronizar.
    pub dial_timeout_secs: u64,
    /// Papel: `auto` ou fixo (`super_peer` no nó headless).
    pub role: String,
    /// Contribuição ligada. Desligada, o papel é sempre `Leaf` (spec 8.3).
    pub contribute: bool,
    /// Modo jogo: força `Leaf` enquanto ligado.
    pub game_mode: bool,
    /// Tempo que a condição precisa se manter para promover a `LeafPlus`/`Peer`/`Relay`.
    pub promote_after_secs: u64,
    /// Idem para `SuperPeer`.
    pub promote_after_super_secs: u64,
    /// Intervalo da leitura de sinais e reavaliação do papel (0 desliga a tarefa).
    pub role_interval_secs: u64,
    /// Limites de contribuição.
    pub budget: Budget,
    /// Aceita sem perguntar qualquer pedido de contato. **Só para nós headless de uso
    /// controlado**: quem tiver o ticket entra.
    pub auto_accept_contacts: bool,
    /// Fingerprints (hex) aceitas sem perguntar.
    pub allowed_fingerprints: Vec<String>,
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
            dns_servers: Vec::new(),
            dns_discovery: true,
            contact_request_timeout_secs: 300,
            role: "auto".to_owned(),
            contribute: true,
            game_mode: false,
            promote_after_secs: 120,
            promote_after_super_secs: 600,
            role_interval_secs: 10,
            budget: Budget::default(),
            auto_accept_contacts: false,
            allowed_fingerprints: Vec::new(),
            dial_timeout_secs: 15,
            sync_interval_secs: 30,
            ticket_ttl_secs: 24 * 3600,
            max_frame_bytes: 1024 * 1024,
        }
    }
}

impl Config {
    /// `dns_servers` como endereços IP, ou erro se algum não for um IP válido.
    pub fn dns_server_addrs(&self) -> Result<Vec<std::net::IpAddr>> {
        self.dns_servers
            .iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| Error::Config(format!("dns_servers: `{s}` não é um IP")))
            })
            .collect()
    }

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
        if let Some(v) = env_nonempty("ENXAME_DISPLAY_NAME") {
            cfg.display_name = v;
        }
        if let Some(v) = env_nonempty("ENXAME_RELAY") {
            cfg.relay = RelaySetting::parse(&v)?;
        }
        if let Some(v) = env_nonempty("ENXAME_RELAY_CA_FILE") {
            cfg.relay_ca_file = Some(PathBuf::from(v));
        }
        if let Some(v) = env_nonempty("ENXAME_MDNS") {
            cfg.mdns = !matches!(v.as_str(), "0" | "false" | "off");
        }
        if let Some(v) = env_nonempty("ENXAME_ROLE") {
            cfg.role = v;
        }
        if let Some(v) = env_nonempty("ENXAME_CONTRIBUTE") {
            cfg.contribute = !matches!(v.as_str(), "0" | "false" | "off");
        }
        if let Some(v) = env_nonempty("ENXAME_GAME_MODE") {
            cfg.game_mode = matches!(v.as_str(), "1" | "true" | "on");
        }
        if let Some(v) = env_nonempty("ENXAME_MAX_CONTRIB_KBPS") {
            cfg.budget.max_contrib_kbps = Some(
                v.parse()
                    .map_err(|_| Error::Config("ENXAME_MAX_CONTRIB_KBPS".into()))?,
            );
        }
        if let Some(v) = env_nonempty("ENXAME_MAX_CONTRIB_GB_MONTH") {
            cfg.budget.max_contrib_gb_month = v
                .parse()
                .map_err(|_| Error::Config("ENXAME_MAX_CONTRIB_GB_MONTH".into()))?;
        }
        if let Some(v) = env_nonempty("ENXAME_CARRY_QUOTA_MB_TOTAL") {
            cfg.budget.carry_quota_mb_total = Some(
                v.parse()
                    .map_err(|_| Error::Config("ENXAME_CARRY_QUOTA_MB_TOTAL".into()))?,
            );
        }
        if let Some(v) = env_nonempty("ENXAME_AUTO_ACCEPT_CONTACTS") {
            cfg.auto_accept_contacts = matches!(v.as_str(), "1" | "true" | "on");
        }
        if let Some(v) = env_nonempty("ENXAME_ALLOWED_FINGERPRINTS") {
            cfg.allowed_fingerprints = v
                .split(',')
                .map(|x| x.trim().to_lowercase())
                .filter(|x| !x.is_empty())
                .collect();
        }
        if let Some(v) = env_nonempty("ENXAME_DNS_SERVERS") {
            cfg.dns_servers = v
                .split(',')
                .map(|x| x.trim().to_owned())
                .filter(|x| !x.is_empty())
                .collect();
        }
        cfg.dns_server_addrs()?;
        RoleMode::parse(&cfg.role)?;
        if let Some(v) = env_nonempty("ENXAME_DNS_DISCOVERY") {
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
            role_interval_secs: 0,
            ..Self::default()
        }
    }
}

/// Variável de ambiente definida e não vazia (`VAR=` conta como ausente, o que facilita o
/// `docker compose` com valores opcionais).
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_servers_must_be_ip_addresses() {
        let mut c = Config {
            dns_servers: vec!["1.1.1.1".into(), "2606:4700:4700::1111".into()],
            ..Config::default()
        };
        assert_eq!(c.dns_server_addrs().unwrap().len(), 2);
        c.dns_servers = vec!["dns.google".into()];
        assert!(c.dns_server_addrs().is_err());
        assert!(Config::default().dns_server_addrs().unwrap().is_empty());
    }
}
