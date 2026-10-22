//! `enxame-node`: o mesmo core do app, sem UI e com papel fixo `SuperPeer` (spec 11.1).
//!
//! Pensado para VPS/homelab 24/7: guarda mensagens offline dos seus contatos por TTL longo e é
//! o carrier preferido. Configuração por `config.toml` no diretório de dados e por `ENXAME_*`.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use enxame_core::{fingerprint, Config, Enxame, Event, FileKeyStore};
use tracing_subscriber::EnvFilter;

const HEALTH_FILE: &str = "health";
const TICKET_FILE: &str = "node-ticket.txt";
const HEALTH_EVERY: Duration = Duration::from_secs(10);
const HEALTH_MAX_AGE: Duration = Duration::from_secs(60);
const TICKET_EVERY: Duration = Duration::from_secs(3600);

/// Nó headless do Enxame.
#[derive(Debug, Parser)]
#[command(name = "enxame-node", version, about)]
struct Cli {
    /// Diretório de dados (padrão: ENXAME_DATA_DIR ou o diretório XDG).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Sobe o nó (padrão).
    Run,
    /// Healthcheck para o Docker: sai com 0 se o nó escreveu o arquivo de saúde há pouco.
    Health,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run(cli.data_dir).await,
        Command::Health => health(cli.data_dir),
    }
}

fn data_dir(arg: Option<PathBuf>) -> Result<PathBuf> {
    Ok(Config::load(arg)?.data_dir)
}

/// 0 = saudável; 1 = arquivo ausente ou velho.
fn health(arg: Option<PathBuf>) -> Result<()> {
    let path = data_dir(arg)?.join(HEALTH_FILE);
    let age = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok());
    match age {
        Some(a) if a <= HEALTH_MAX_AGE => Ok(()),
        Some(a) => bail!("sem sinal de vida há {}s", a.as_secs()),
        None => bail!("{} não existe: o nó ainda não subiu", path.display()),
    }
}

fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(text.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

async fn run(arg: Option<PathBuf>) -> Result<()> {
    let mut cfg = Config::load(arg)?;
    // O nó é sempre SuperPeer e contribui; o budget e as quotas vêm da configuração.
    cfg.role = "super_peer".into();
    cfg.contribute = true;
    cfg.game_mode = false;
    let dir = cfg.data_dir.clone();
    std::fs::create_dir_all(&dir)?;
    let store = FileKeyStore::new(dir.join("identity.key"));
    let node = Enxame::start(cfg.clone(), &store).await?;
    tracing::info!(
        fingerprint = %fingerprint(&node.identity_pk()),
        node_id = %node.endpoint_id(),
        role = ?node.role(),
        budget_gb_month = cfg.budget.max_contrib_gb_month,
        "enxame-node no ar"
    );
    if !cfg.auto_accept_contacts && cfg.allowed_fingerprints.is_empty() {
        tracing::warn!(
            "nenhum contato será aceito: defina ENXAME_ALLOWED_FINGERPRINTS (ou allowed_fingerprints \
             no config.toml) com as fingerprints de quem pode usar este nó"
        );
    }

    let mut events = node.subscribe();
    let mut health_tick = tokio::time::interval(HEALTH_EVERY);
    let mut ticket_tick = tokio::time::interval(TICKET_EVERY);
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("SIGTERM")?;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = term.recv() => break,
            _ = ticket_tick.tick() => match node.invite_ticket().await {
                Ok(t) => {
                    write_private(&dir.join(TICKET_FILE), &format!("{t}\n"))?;
                    tracing::info!(file = %dir.join(TICKET_FILE).display(), "ticket do nó atualizado");
                }
                Err(e) => tracing::warn!("não foi possível gerar o ticket: {e}"),
            },
            _ = health_tick.tick() => {
                let summary = node.contribution_summary().await.ok();
                let line = format!(
                    "ok role={:?} contacts={} carry_items={} bytes_served_month={}\n",
                    node.role(),
                    node.contacts().iter().filter(|c| c.is_mutual()).count(),
                    summary.as_ref().map_or(0, |s| s.carry_items),
                    summary.as_ref().map_or(0, |s| s.bytes_served),
                );
                write_private(&dir.join(HEALTH_FILE), &line)?;
            }
            ev = events.recv() => match ev {
                Ok(Event::ContactRequested { request_id, display_name, fingerprint }) => {
                    // Sem humano para perguntar: recusa e diz como liberar.
                    let _ = node.respond_contact(request_id, false);
                    tracing::warn!(
                        %display_name, %fingerprint,
                        "pedido de contato recusado; para liberar, adicione a fingerprint a ENXAME_ALLOWED_FINGERPRINTS"
                    );
                }
                Ok(Event::ContactAdded { display_name, fingerprint }) => {
                    tracing::info!(%display_name, %fingerprint, "contato mútuo");
                }
                Ok(Event::RoleChanged { from, to }) => tracing::info!(?from, ?to, "papel mudou"),
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
        }
    }
    tracing::info!("encerrando");
    node.shutdown().await;
    Ok(())
}
