//! Papéis e avaliação de capacidade (spec 8): sinais, regra de decisão, histerese e budget.
//!
//! Tudo aqui é determinístico e recebe o tempo como parâmetro, para ser testado com relógio
//! simulado. A leitura dos sinais do SO fica atrás de [`PlatformSignals`], de modo que
//! Android/iOS (e Windows) plugam implementações próprias sem tocar na lógica.

use std::{sync::Mutex, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{config::Budget, net::NatKind, Role};

/// Tipo de enlace de saída (spec 8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LinkKind {
    Ethernet,
    Wifi,
    Cellular,
    #[default]
    Unknown,
}

/// O que a plataforma sabe medir. Valores desconhecidos são `None` ou o lado conservador.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformReading {
    pub on_ac_power: bool,
    pub battery_pct: Option<u8>,
    pub metered: Option<bool>,
    pub link_kind: LinkKind,
    pub thermal_ok: bool,
    /// Fração de CPU livre, 0.0–1.0.
    pub cpu_free: Option<f32>,
    pub mem_free_mb: Option<u64>,
}

impl Default for PlatformReading {
    /// Sem informação: desktop na tomada, enlace desconhecido (o que limita a `Peer`).
    fn default() -> Self {
        Self {
            on_ac_power: true,
            battery_pct: None,
            metered: None,
            link_kind: LinkKind::Unknown,
            thermal_ok: true,
            cpu_free: None,
            mem_free_mb: None,
        }
    }
}

/// Fonte de sinais do SO. Implementada por plataforma, fora da lógica central.
pub trait PlatformSignals: Send + Sync {
    fn read(&self) -> PlatformReading;
}

/// Sinais completos usados na decisão (spec 8.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Signals {
    pub nat_kind: NatKind,
    /// Estimativa passiva de upload (EWMA das transferências reais); `None` até haver amostra.
    pub upload_kbps_est: Option<u64>,
    pub platform: PlatformReading,
    pub session_uptime: Duration,
    /// Reservado para mobile; no desktop é sempre `true`.
    pub foreground: bool,
    pub game_mode: bool,
}

/// Escolhas do usuário e do budget que entram na decisão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RolePolicy {
    pub contribution_enabled: bool,
    pub budget_exceeded: bool,
}

impl Default for RolePolicy {
    fn default() -> Self {
        Self {
            contribution_enabled: true,
            budget_exceeded: false,
        }
    }
}

const BATTERY_LOW_PCT: u8 = 20;
const SUPERPEER_MIN_UPLOAD_KBPS: u64 = 5000;
const SUPERPEER_MIN_UPTIME: Duration = Duration::from_secs(10 * 60);

/// Papel-alvo para os sinais atuais (regra da spec 8.3), antes da histerese.
///
/// Leitura conservadora de "sinal desconhecido": enlace `Unknown` limita o papel a `Peer`
/// (não se faz relay pesado sem saber o que há embaixo). `metered` desconhecido conta como não
/// medido.
pub fn target_role(s: &Signals, p: &RolePolicy) -> Role {
    let pf = &s.platform;
    let ac = pf.on_ac_power;
    let mut role = if s.game_mode || !p.contribution_enabled {
        Role::Leaf
    } else if pf.link_kind == LinkKind::Cellular || pf.metered == Some(true) {
        Role::LeafPlus
    } else if (pf.battery_pct.is_some_and(|b| b < BATTERY_LOW_PCT) && !ac) || !pf.thermal_ok {
        // Bateria baixa fora da tomada ou superaquecimento.
        Role::Leaf
    } else if pf.link_kind == LinkKind::Wifi && !ac {
        Role::Peer
    } else if ac
        && matches!(s.nat_kind, NatKind::Open | NatKind::Cone)
        && s.upload_kbps_est
            .is_some_and(|k| k >= SUPERPEER_MIN_UPLOAD_KBPS)
        && s.session_uptime >= SUPERPEER_MIN_UPTIME
    {
        Role::SuperPeer
    } else if ac {
        Role::Relay
    } else {
        Role::Peer
    };
    if pf.link_kind == LinkKind::Unknown {
        role = role.min(Role::Peer);
    }
    if p.budget_exceeded {
        role = role.min(Role::LeafPlus);
    }
    role
}

/// Histerese: promoção só depois de a condição se manter por `promote_after`; rebaixamento
/// imediato (spec 8.3). O tempo é um `Duration` monotônico fornecido por quem chama.
#[derive(Debug, Clone)]
pub struct RoleEngine {
    current: Role,
    candidate: Option<(Role, Duration)>,
    promote_after: Duration,
    promote_after_super: Duration,
}

impl RoleEngine {
    /// Começa em `Leaf`: nada é assumido antes de a condição provar-se estável.
    pub fn new(promote_after: Duration, promote_after_super: Duration) -> Self {
        Self {
            current: Role::Leaf,
            candidate: None,
            promote_after,
            promote_after_super,
        }
    }

    pub fn current(&self) -> Role {
        self.current
    }

    fn wait_for(&self, target: Role) -> Duration {
        if target == Role::SuperPeer {
            self.promote_after_super
        } else {
            self.promote_after
        }
    }

    /// Reavalia com o papel-alvo atual. Devolve `(antes, depois)` se o papel mudou.
    pub fn update(&mut self, target: Role, now: Duration) -> Option<(Role, Role)> {
        let before = self.current;
        if target <= self.current {
            self.candidate = None;
            self.current = target;
        } else {
            match self.candidate {
                Some((role, since)) if role == target => {
                    if now.saturating_sub(since) >= self.wait_for(target) {
                        self.current = target;
                        self.candidate = None;
                    }
                }
                _ => {
                    self.candidate = Some((target, now));
                    if self.wait_for(target).is_zero() {
                        self.current = target;
                        self.candidate = None;
                    }
                }
            }
        }
        (self.current != before).then_some((before, self.current))
    }
}

/// Estimativa passiva de upload: EWMA do throughput observado em transferências reais.
/// Nunca faz speedtest ativo (spec 8.1).
#[derive(Debug, Default)]
pub struct ThroughputEstimator {
    kbps: Mutex<Option<f64>>,
}

const EWMA_ALPHA: f64 = 0.2;
/// Amostras menores que isto são ruído de bufferização, não de enlace.
const MIN_SAMPLE_BYTES: usize = 16 * 1024;
const MIN_SAMPLE_TIME: Duration = Duration::from_millis(5);

impl ThroughputEstimator {
    /// Registra `bytes` enviados em `elapsed`.
    pub fn record(&self, bytes: usize, elapsed: Duration) {
        if bytes < MIN_SAMPLE_BYTES || elapsed < MIN_SAMPLE_TIME {
            return;
        }
        let sample = (bytes as f64 * 8.0 / 1000.0) / elapsed.as_secs_f64();
        let mut k = self.kbps.lock().unwrap_or_else(|p| p.into_inner());
        *k = Some(match *k {
            Some(prev) => prev * (1.0 - EWMA_ALPHA) + sample * EWMA_ALPHA,
            None => sample,
        });
    }

    pub fn kbps(&self) -> Option<u64> {
        self.kbps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .map(|v| v as u64)
    }
}

/// Limitador de banda de contribuição (balde de fichas simples).
#[derive(Debug)]
pub struct RateLimiter {
    bytes_per_sec: Option<f64>,
    next_free: Mutex<tokio::time::Instant>,
}

impl RateLimiter {
    /// `kbps = None` não limita.
    pub fn new(kbps: Option<u64>) -> Self {
        Self {
            bytes_per_sec: kbps.filter(|k| *k > 0).map(|k| k as f64 * 1000.0 / 8.0),
            next_free: Mutex::new(tokio::time::Instant::now()),
        }
    }

    /// Espera o tempo necessário para que enviar `bytes` respeite a taxa.
    pub async fn acquire(&self, bytes: usize) {
        let Some(rate) = self.bytes_per_sec else {
            return;
        };
        let wait_until = {
            let mut next = self.next_free.lock().unwrap_or_else(|p| p.into_inner());
            let now = tokio::time::Instant::now();
            let start = (*next).max(now);
            *next = start + Duration::from_secs_f64(bytes as f64 / rate);
            start
        };
        tokio::time::sleep_until(wait_until).await;
    }
}

/// Teto efetivo de banda: o configurado, ou 30 % da estimativa passiva (spec 8.3).
pub fn effective_kbps_cap(budget: &Budget, upload_est: Option<u64>) -> Option<u64> {
    budget
        .max_contrib_kbps
        .or_else(|| upload_est.map(|k| (k * 30 / 100).max(1)))
}

/// Limites de carry por papel (spec 8.2/9). `None` em `per_contact`/`total` significa "sem carry".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarryLimits {
    pub accepts: bool,
    /// Aceita carregar envelopes `FileRef` (e replicar o blob): `Relay` e `SuperPeer`.
    pub accepts_files: bool,
    pub per_contact_bytes: u64,
    pub total_bytes: u64,
    pub ttl: Duration,
}

const MB: u64 = 1024 * 1024;
const DAY: Duration = Duration::from_secs(24 * 3600);

impl CarryLimits {
    /// Limites do papel, com os overrides do budget do usuário.
    pub fn for_role(role: Role, budget: &Budget) -> Self {
        let (accepts, per, total, ttl_days) = match role {
            Role::Leaf => (false, 0, 0, 7),
            Role::LeafPlus => (true, 1, 5, 7),
            Role::Peer => (true, 10, 100, 7),
            Role::Relay => (true, 25, 250, 7),
            Role::SuperPeer => (true, 100, 2000, 30),
        };
        Self {
            accepts,
            accepts_files: matches!(role, Role::Relay | Role::SuperPeer),
            per_contact_bytes: budget.carry_quota_mb_per_contact.unwrap_or(per) * MB,
            total_bytes: budget.carry_quota_mb_total.unwrap_or(total) * MB,
            ttl: DAY * ttl_days,
        }
    }
}

/// Fonte de sinais sem informação além de CPU/memória (`sysinfo`). Base para plataformas sem
/// implementação específica; o papel fica limitado a `Peer` (enlace desconhecido).
pub struct FallbackSignals {
    sys: Mutex<sysinfo::System>,
}

impl FallbackSignals {
    pub fn new() -> Self {
        Self {
            sys: Mutex::new(sysinfo::System::new()),
        }
    }

    fn cpu_mem(&self) -> (Option<f32>, Option<u64>) {
        let mut sys = self.sys.lock().unwrap_or_else(|p| p.into_inner());
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let cpu_free = Some((1.0 - sys.global_cpu_usage() / 100.0).clamp(0.0, 1.0));
        (cpu_free, Some(sys.available_memory() / MB))
    }
}

impl Default for FallbackSignals {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformSignals for FallbackSignals {
    fn read(&self) -> PlatformReading {
        let (cpu_free, mem_free_mb) = self.cpu_mem();
        PlatformReading {
            cpu_free,
            mem_free_mb,
            ..PlatformReading::default()
        }
    }
}

/// Sinais fixos e alteráveis em tempo de execução (testes e integradores).
#[derive(Debug, Default)]
pub struct StaticSignals {
    reading: Mutex<PlatformReading>,
}

impl StaticSignals {
    pub fn new(reading: PlatformReading) -> Self {
        Self {
            reading: Mutex::new(reading),
        }
    }

    /// Altera a leitura devolvida a partir de agora.
    pub fn set(&self, reading: PlatformReading) {
        *self.reading.lock().unwrap_or_else(|p| p.into_inner()) = reading;
    }
}

impl PlatformSignals for StaticSignals {
    fn read(&self) -> PlatformReading {
        self.reading
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

#[cfg(target_os = "linux")]
pub use linux::LinuxSignals;
#[cfg(windows)]
pub use windows::WindowsSignals;

/// Uma bateria como o SO a informa (independente de plataforma, para poder testar).
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BatteryInfo {
    pub discharging: bool,
    pub pct: u8,
}

/// `(na_tomada, bateria_%)` a partir das baterias: sem bateria = desktop na tomada; com bateria, só
/// está "na tomada" se nenhuma estiver descarregando; o percentual é o da mais baixa.
#[cfg(any(windows, test))]
pub(crate) fn power_from_batteries(batteries: &[BatteryInfo]) -> (bool, Option<u8>) {
    if batteries.is_empty() {
        return (true, None);
    }
    let on_ac = !batteries.iter().any(|b| b.discharging);
    (on_ac, batteries.iter().map(|b| b.pct).min())
}

/// Leitura de sinais no Windows: CPU e memória como no resto, bateria pela API do SO. Enlace e
/// rede medida ficam desconhecidos (o que limita o papel a `Peer`, spec 8.2).
#[cfg(windows)]
mod windows {
    use super::{
        power_from_batteries, BatteryInfo, FallbackSignals, PlatformReading, PlatformSignals,
    };

    pub struct WindowsSignals {
        base: FallbackSignals,
    }

    impl WindowsSignals {
        pub fn new() -> Self {
            Self {
                base: FallbackSignals::new(),
            }
        }
    }

    impl Default for WindowsSignals {
        fn default() -> Self {
            Self::new()
        }
    }

    fn batteries() -> Vec<BatteryInfo> {
        let Ok(manager) = starship_battery::Manager::new() else {
            return Vec::new();
        };
        let Ok(list) = manager.batteries() else {
            return Vec::new();
        };
        list.flatten()
            .map(|b| BatteryInfo {
                discharging: matches!(b.state(), starship_battery::State::Discharging),
                pct: b
                    .state_of_charge()
                    .get::<starship_battery::units::ratio::percent>()
                    .round()
                    .clamp(0.0, 100.0) as u8,
            })
            .collect()
    }

    impl PlatformSignals for WindowsSignals {
        fn read(&self) -> PlatformReading {
            let mut r = self.base.read();
            let (ac, pct) = power_from_batteries(&batteries());
            r.on_ac_power = ac;
            r.battery_pct = pct;
            r
        }
    }
}

/// Leitura de sinais no Linux: `/sys/class/power_supply`, `/proc/net/route`, `/sys/class/net`
/// e NetworkManager (via `nmcli`, se existir). Cada leitura que falha cai no valor conservador.
#[cfg(target_os = "linux")]
mod linux {
    use std::{
        path::{Path, PathBuf},
        process::{Command, Stdio},
    };

    use super::{FallbackSignals, LinkKind, PlatformReading, PlatformSignals};

    pub struct LinuxSignals {
        base: FallbackSignals,
        sys_root: PathBuf,
        route_file: PathBuf,
    }

    impl LinuxSignals {
        pub fn new() -> Self {
            Self::with_roots(PathBuf::from("/sys"), PathBuf::from("/proc/net/route"))
        }

        /// Raízes configuráveis para testar com diretórios falsos.
        pub fn with_roots(sys_root: PathBuf, route_file: PathBuf) -> Self {
            Self {
                base: FallbackSignals::new(),
                sys_root,
                route_file,
            }
        }
    }

    impl Default for LinuxSignals {
        fn default() -> Self {
            Self::new()
        }
    }

    impl PlatformSignals for LinuxSignals {
        fn read(&self) -> PlatformReading {
            let mut r = self.base.read();
            let (ac, battery) = read_power(&self.sys_root.join("class/power_supply"));
            r.on_ac_power = ac;
            r.battery_pct = battery;
            let iface = std::fs::read_to_string(&self.route_file)
                .ok()
                .and_then(|t| default_route_iface(&t));
            if let Some(iface) = &iface {
                let wireless = self
                    .sys_root
                    .join("class/net")
                    .join(iface)
                    .join("wireless")
                    .exists();
                r.link_kind = classify_iface(iface, wireless);
                r.metered = nm_metered(iface);
            }
            r
        }
    }

    /// `(na_tomada, bateria_%)`. Sem bateria = na tomada.
    pub(crate) fn read_power(dir: &Path) -> (bool, Option<u8>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return (true, None);
        };
        let (mut has_battery, mut mains_online, mut pct) = (false, false, None);
        for e in entries.flatten() {
            let p = e.path();
            let read = |f: &str| {
                std::fs::read_to_string(p.join(f))
                    .ok()
                    .map(|s| s.trim().to_owned())
            };
            match read("type").as_deref() {
                Some("Battery") => {
                    has_battery = true;
                    if pct.is_none() {
                        pct = read("capacity").and_then(|c| c.parse::<u8>().ok());
                    }
                }
                Some("Mains") => mains_online |= read("online").as_deref() == Some("1"),
                _ => {}
            }
        }
        (!has_battery || mains_online, pct)
    }

    /// Interface da rota padrão (menor métrica) a partir do conteúdo de `/proc/net/route`.
    pub(crate) fn default_route_iface(route: &str) -> Option<String> {
        route
            .lines()
            .skip(1)
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                let (iface, dest, flags, metric) = (f.first()?, f.get(1)?, f.get(3)?, f.get(6)?);
                let up = u32::from_str_radix(flags, 16).ok()? & 0x1 != 0;
                (*dest == "00000000" && up)
                    .then(|| (metric.parse::<u32>().unwrap_or(u32::MAX), iface.to_string()))
            })
            .min()
            .map(|(_, i)| i)
    }

    pub(crate) fn classify_iface(name: &str, wireless: bool) -> LinkKind {
        if wireless || name.starts_with("wl") {
            LinkKind::Wifi
        } else if name.starts_with("ww") || name.starts_with("rmnet") || name.starts_with("ccmni") {
            LinkKind::Cellular
        } else if name.starts_with("en") || name.starts_with("eth") {
            LinkKind::Ethernet
        } else {
            LinkKind::Unknown
        }
    }

    pub(crate) fn parse_nm_metered(out: &str) -> Option<bool> {
        match out.trim() {
            "yes" | "guess-yes" => Some(true),
            "no" | "guess-no" => Some(false),
            _ => None,
        }
    }

    /// Pergunta ao NetworkManager se o enlace é medido. `None` se não houver `nmcli`.
    fn nm_metered(iface: &str) -> Option<bool> {
        let out = Command::new("nmcli")
            .args(["-t", "-g", "GENERAL.METERED", "device", "show", iface])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| parse_nm_metered(&String::from_utf8_lossy(&out.stdout)))
            .flatten()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn write(dir: &Path, rel: &str, content: &str) {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }

        #[test]
        fn power_without_battery_is_ac() {
            let d = tempfile::tempdir().unwrap();
            write(d.path(), "AC/type", "Mains\n");
            write(d.path(), "AC/online", "1\n");
            assert_eq!(read_power(d.path()), (true, None));
            assert_eq!(read_power(&d.path().join("nao-existe")), (true, None));
        }

        #[test]
        fn laptop_on_battery_and_on_charger() {
            let d = tempfile::tempdir().unwrap();
            write(d.path(), "BAT0/type", "Battery\n");
            write(d.path(), "BAT0/capacity", "17\n");
            write(d.path(), "AC/type", "Mains\n");
            write(d.path(), "AC/online", "0\n");
            assert_eq!(read_power(d.path()), (false, Some(17)));
            write(d.path(), "AC/online", "1\n");
            assert_eq!(read_power(d.path()), (true, Some(17)));
        }

        #[test]
        fn default_route_picks_lowest_metric_up_route() {
            let t = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
                     wlan0\t00000000\t0100A8C0\t0003\t0\t0\t600\t00000000\n\
                     enp3s0\t00000000\t0100A8C0\t0003\t0\t0\t100\t00000000\n\
                     enp3s0\t0000A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\n";
            assert_eq!(default_route_iface(t).as_deref(), Some("enp3s0"));
            assert_eq!(default_route_iface("Iface\n"), None);
        }

        #[test]
        fn interface_classification() {
            assert_eq!(classify_iface("wlan0", false), LinkKind::Wifi);
            assert_eq!(classify_iface("foo0", true), LinkKind::Wifi);
            assert_eq!(classify_iface("enp3s0", false), LinkKind::Ethernet);
            assert_eq!(classify_iface("wwan0", false), LinkKind::Cellular);
            assert_eq!(classify_iface("tailscale0", false), LinkKind::Unknown);
        }

        #[test]
        fn networkmanager_metered_values() {
            assert_eq!(parse_nm_metered("yes\n"), Some(true));
            assert_eq!(parse_nm_metered("guess-yes"), Some(true));
            assert_eq!(parse_nm_metered("no"), Some(false));
            assert_eq!(parse_nm_metered("guess-no"), Some(false));
            assert_eq!(parse_nm_metered("unknown"), None);
            assert_eq!(parse_nm_metered(""), None);
        }

        #[test]
        fn linux_signals_read_from_fake_roots() {
            let d = tempfile::tempdir().unwrap();
            write(d.path(), "class/power_supply/BAT0/type", "Battery\n");
            write(d.path(), "class/power_supply/BAT0/capacity", "55\n");
            write(d.path(), "class/power_supply/AC/type", "Mains\n");
            write(d.path(), "class/power_supply/AC/online", "0\n");
            write(d.path(), "class/net/wlan0/wireless/.keep", "");
            let route = d.path().join("route");
            std::fs::write(&route, "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\nwlan0\t00000000\t0100A8C0\t0003\t0\t0\t600\t00000000\n").unwrap();
            let s = LinuxSignals::with_roots(d.path().to_path_buf(), route);
            let r = s.read();
            assert!(!r.on_ac_power);
            assert_eq!(r.battery_pct, Some(55));
            assert_eq!(r.link_kind, LinkKind::Wifi);
            assert!(r.mem_free_mb.is_some());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn power_from_batteries_covers_desktops_laptops_and_unplugging() {
        use super::{power_from_batteries, BatteryInfo};
        let b = |discharging, pct| BatteryInfo { discharging, pct };
        // Desktop: sem bateria, na tomada.
        assert_eq!(power_from_batteries(&[]), (true, None));
        // Notebook carregando ou cheio: na tomada, com o percentual.
        assert_eq!(power_from_batteries(&[b(false, 80)]), (true, Some(80)));
        // Notebook na bateria: fora da tomada (o papel cai).
        assert_eq!(power_from_batteries(&[b(true, 55)]), (false, Some(55)));
        // Duas baterias: basta uma descarregando; vale o menor percentual.
        assert_eq!(
            power_from_batteries(&[b(false, 90), b(true, 40)]),
            (false, Some(40))
        );
    }

    use super::*;

    fn base() -> Signals {
        Signals {
            nat_kind: NatKind::Cone,
            upload_kbps_est: Some(20_000),
            platform: PlatformReading {
                on_ac_power: true,
                battery_pct: None,
                metered: Some(false),
                link_kind: LinkKind::Ethernet,
                thermal_ok: true,
                cpu_free: Some(0.9),
                mem_free_mb: Some(8000),
            },
            session_uptime: Duration::from_secs(3600),
            foreground: true,
            game_mode: false,
        }
    }

    fn role(f: impl FnOnce(&mut Signals)) -> Role {
        let mut s = base();
        f(&mut s);
        target_role(&s, &RolePolicy::default())
    }

    #[test]
    fn decision_table() {
        // (descrição, mutação, papel esperado)
        type Mutation = Box<dyn Fn(&mut Signals)>;
        let cases: Vec<(&str, Mutation, Role)> = vec![
            (
                "desktop cabeado, NAT cone, bom upload",
                Box::new(|_| {}),
                Role::SuperPeer,
            ),
            (
                "NAT aberto",
                Box::new(|s| s.nat_kind = NatKind::Open),
                Role::SuperPeer,
            ),
            (
                "NAT simétrico → Relay",
                Box::new(|s| s.nat_kind = NatKind::Symmetric),
                Role::Relay,
            ),
            (
                "NAT desconhecido → Relay",
                Box::new(|s| s.nat_kind = NatKind::Unknown),
                Role::Relay,
            ),
            (
                "upload baixo → Relay",
                Box::new(|s| s.upload_kbps_est = Some(1000)),
                Role::Relay,
            ),
            (
                "upload ainda sem amostra → Relay",
                Box::new(|s| s.upload_kbps_est = None),
                Role::Relay,
            ),
            (
                "upload exatamente no limite",
                Box::new(|s| s.upload_kbps_est = Some(5000)),
                Role::SuperPeer,
            ),
            (
                "pouco uptime → Relay",
                Box::new(|s| s.session_uptime = Duration::from_secs(599)),
                Role::Relay,
            ),
            (
                "uptime exatamente 10 min",
                Box::new(|s| s.session_uptime = Duration::from_secs(600)),
                Role::SuperPeer,
            ),
            (
                "modo jogo → Leaf",
                Box::new(|s| s.game_mode = true),
                Role::Leaf,
            ),
            (
                "celular → LeafPlus",
                Box::new(|s| s.platform.link_kind = LinkKind::Cellular),
                Role::LeafPlus,
            ),
            (
                "rede medida → LeafPlus",
                Box::new(|s| s.platform.metered = Some(true)),
                Role::LeafPlus,
            ),
            (
                "rede medida desconhecida não rebaixa",
                Box::new(|s| s.platform.metered = None),
                Role::SuperPeer,
            ),
            (
                "bateria < 20% fora da tomada → Leaf",
                Box::new(|s| {
                    s.platform.on_ac_power = false;
                    s.platform.battery_pct = Some(19);
                }),
                Role::Leaf,
            ),
            (
                "bateria 20% fora da tomada, ethernet → Peer",
                Box::new(|s| {
                    s.platform.on_ac_power = false;
                    s.platform.battery_pct = Some(20);
                }),
                Role::Peer,
            ),
            (
                "bateria baixa na tomada segue",
                Box::new(|s| s.platform.battery_pct = Some(5)),
                Role::SuperPeer,
            ),
            (
                "superaquecido → Leaf",
                Box::new(|s| s.platform.thermal_ok = false),
                Role::Leaf,
            ),
            (
                "notebook no wifi na bateria → Peer",
                Box::new(|s| {
                    s.platform.link_kind = LinkKind::Wifi;
                    s.platform.on_ac_power = false;
                    s.platform.battery_pct = Some(80);
                }),
                Role::Peer,
            ),
            (
                "wifi na tomada",
                Box::new(|s| s.platform.link_kind = LinkKind::Wifi),
                Role::SuperPeer,
            ),
            (
                "enlace desconhecido limita a Peer",
                Box::new(|s| s.platform.link_kind = LinkKind::Unknown),
                Role::Peer,
            ),
            (
                "celular vence bateria baixa e superaquecimento por ordem da regra",
                Box::new(|s| {
                    s.platform.link_kind = LinkKind::Cellular;
                    s.platform.on_ac_power = false;
                    s.platform.battery_pct = Some(5);
                }),
                Role::LeafPlus,
            ),
        ];
        for (name, f, want) in cases {
            assert_eq!(role(&*f), want, "{name}");
        }
    }

    #[test]
    fn user_switches_and_budget() {
        let s = base();
        let off = RolePolicy {
            contribution_enabled: false,
            budget_exceeded: false,
        };
        assert_eq!(target_role(&s, &off), Role::Leaf);
        let over = RolePolicy {
            contribution_enabled: true,
            budget_exceeded: true,
        };
        assert_eq!(target_role(&s, &over), Role::LeafPlus);
        // Budget estourado nunca sobe quem já é Leaf.
        let mut g = base();
        g.game_mode = true;
        assert_eq!(target_role(&g, &over), Role::Leaf);
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn promotion_is_delayed_and_demotion_is_immediate() {
        let mut e = RoleEngine::new(secs(120), secs(600));
        assert_eq!(e.current(), Role::Leaf);
        // Relay: precisa de 120 s estável.
        assert_eq!(e.update(Role::Relay, secs(0)), None);
        assert_eq!(e.update(Role::Relay, secs(119)), None);
        assert_eq!(e.current(), Role::Leaf);
        assert_eq!(
            e.update(Role::Relay, secs(120)),
            Some((Role::Leaf, Role::Relay))
        );
        // SuperPeer: 600 s.
        assert_eq!(e.update(Role::SuperPeer, secs(200)), None);
        assert_eq!(e.update(Role::SuperPeer, secs(799)), None);
        assert_eq!(
            e.update(Role::SuperPeer, secs(800)),
            Some((Role::Relay, Role::SuperPeer))
        );
        // Rebaixamento imediato, sem esperar nada.
        assert_eq!(
            e.update(Role::Leaf, secs(801)),
            Some((Role::SuperPeer, Role::Leaf))
        );
        assert_eq!(e.current(), Role::Leaf);
    }

    #[test]
    fn unstable_condition_restarts_the_timer() {
        let mut e = RoleEngine::new(secs(120), secs(600));
        e.update(Role::Peer, secs(0));
        e.update(Role::Peer, secs(100));
        // A condição some por um instante (cai para Leaf) e volta: recomeça do zero.
        assert_eq!(e.update(Role::Leaf, secs(110)), None);
        e.update(Role::Peer, secs(115));
        assert_eq!(
            e.update(Role::Peer, secs(200)),
            None,
            "só 85 s desde a retomada"
        );
        assert_eq!(
            e.update(Role::Peer, secs(235)),
            Some((Role::Leaf, Role::Peer))
        );
    }

    #[test]
    fn changing_target_while_waiting_restarts_the_timer() {
        let mut e = RoleEngine::new(secs(120), secs(600));
        e.update(Role::SuperPeer, secs(0));
        e.update(Role::Relay, secs(500)); // alvo mudou: novo relógio
        assert_eq!(e.update(Role::Relay, secs(619)), None);
        assert_eq!(
            e.update(Role::Relay, secs(620)),
            Some((Role::Leaf, Role::Relay))
        );
    }

    #[test]
    fn partial_demotion_is_immediate_too() {
        let mut e = RoleEngine::new(secs(0), secs(0));
        assert_eq!(
            e.update(Role::SuperPeer, secs(0)),
            Some((Role::Leaf, Role::SuperPeer))
        );
        assert_eq!(
            e.update(Role::Peer, secs(1)),
            Some((Role::SuperPeer, Role::Peer))
        );
    }

    #[test]
    fn throughput_estimator_ignores_noise_and_smooths() {
        let t = ThroughputEstimator::default();
        t.record(100, secs(1));
        t.record(1_000_000, Duration::from_millis(1));
        assert_eq!(t.kbps(), None);
        // 1 MB em 1 s = 8000 kbps.
        t.record(1_000_000, secs(1));
        assert_eq!(t.kbps(), Some(8000));
        // Uma amostra lenta puxa a média só em parte (alpha 0,2).
        t.record(125_000, secs(1)); // 1000 kbps
        assert_eq!(t.kbps(), Some(6600));
    }

    #[test]
    fn kbps_cap_defaults_to_30_percent_of_estimate() {
        let mut b = Budget::default();
        assert_eq!(effective_kbps_cap(&b, Some(10_000)), Some(3000));
        assert_eq!(effective_kbps_cap(&b, None), None);
        b.max_contrib_kbps = Some(500);
        assert_eq!(effective_kbps_cap(&b, Some(10_000)), Some(500));
    }

    #[test]
    fn carry_limits_by_role() {
        let b = Budget::default();
        assert!(!CarryLimits::for_role(Role::Leaf, &b).accepts);
        let lp = CarryLimits::for_role(Role::LeafPlus, &b);
        let sp = CarryLimits::for_role(Role::SuperPeer, &b);
        assert!(lp.accepts && sp.accepts);
        assert!(lp.total_bytes < CarryLimits::for_role(Role::Peer, &b).total_bytes);
        assert!(sp.total_bytes > CarryLimits::for_role(Role::Relay, &b).total_bytes);
        assert_eq!(lp.ttl, secs(7 * 24 * 3600));
        assert_eq!(sp.ttl, secs(30 * 24 * 3600));
        let custom = Budget {
            carry_quota_mb_total: Some(1),
            carry_quota_mb_per_contact: Some(1),
            ..Budget::default()
        };
        assert_eq!(
            CarryLimits::for_role(Role::SuperPeer, &custom).total_bytes,
            1024 * 1024
        );
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limiter_spaces_out_sends() {
        // 8 kbps = 1000 bytes/s.
        let rl = RateLimiter::new(Some(8));
        let t0 = tokio::time::Instant::now();
        rl.acquire(1000).await; // sai na hora, reserva 1 s
        rl.acquire(1000).await; // espera 1 s
        rl.acquire(500).await; // espera mais 1 s
        assert_eq!(t0.elapsed(), secs(2));
        let free = RateLimiter::new(None);
        let t1 = tokio::time::Instant::now();
        free.acquire(10_000_000).await;
        assert_eq!(t1.elapsed(), Duration::ZERO);
    }

    #[test]
    fn role_mode_parsing() {
        use crate::config::RoleMode;
        assert_eq!(RoleMode::parse("auto").unwrap(), RoleMode::Auto);
        assert_eq!(
            RoleMode::parse("super-peer").unwrap(),
            RoleMode::Fixed(Role::SuperPeer)
        );
        assert_eq!(
            RoleMode::parse("LeafPlus").unwrap(),
            RoleMode::Fixed(Role::LeafPlus)
        );
        assert!(RoleMode::parse("rei").is_err());
    }
}
