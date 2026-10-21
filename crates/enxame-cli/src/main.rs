use std::{path::PathBuf, time::Duration};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use enxame_core::{
    fingerprint, open_keystore, AddOutcome, Config, DiagReport, Enxame, Event, PathKind,
    RelaySetting,
};
use tracing_subscriber::EnvFilter;

/// Enxame: comunicador P2P friend-to-friend.
#[derive(Debug, Parser)]
#[command(name = "enxame-cli", version, about)]
struct Cli {
    /// Diretório de dados (padrão: ENXAME_DATA_DIR ou o diretório XDG/APPDATA).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Relay: `default`, `disabled` ou uma URL (padrão: config.toml / ENXAME_RELAY).
    #[arg(long, global = true)]
    relay: Option<String>,
    /// Desliga a descoberta mDNS.
    #[arg(long, global = true)]
    no_mdns: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Mostra a versão do core e do protocolo.
    Version,
    /// Gera a identidade (se ainda não existir) e mostra seus dados.
    Init {
        /// Nome exibido nos tickets (salvo em config.toml se informado).
        #[arg(long)]
        name: Option<String>,
    },
    /// Mostra a identidade e o endereço deste dispositivo.
    Id,
    /// Gera um ticket de convite (texto `enxame1...`).
    Ticket {
        /// Mostra também o QR em texto.
        #[arg(long)]
        qr: bool,
    },
    /// Importa um ticket e pede para virar contato; espera o outro lado aceitar.
    Add { ticket: String },
    /// Fica online aceitando conexões e pedidos de contato.
    Serve {
        /// Aceita todos os pedidos sem perguntar (somente desenvolvimento).
        #[arg(long)]
        auto_accept: bool,
    },
    /// Lista os contatos.
    Contacts,
    /// Diagnóstico de rede: NAT, caminho (direto/relay), RTT, papel.
    Diag {
        /// Contato (nome ou prefixo da fingerprint); padrão: todos os mútuos.
        contact: Option<String>,
        /// Segundos esperando o caminho virar direto (hole punching) antes de reportar.
        #[arg(long, default_value_t = 10)]
        wait_direct: u64,
    },
}

fn load_config(cli: &Cli) -> Result<Config> {
    let mut cfg = Config::load(cli.data_dir.clone())?;
    if let Some(r) = &cli.relay {
        cfg.relay = RelaySetting::parse(r)?;
    }
    if cli.no_mdns {
        cfg.mdns = false;
    }
    Ok(cfg)
}

async fn start(cfg: Config) -> Result<Enxame> {
    let store = open_keystore(&cfg);
    Ok(Enxame::start(cfg, store.as_ref()).await?)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match &cli.command {
        Command::Version => {
            println!(
                "enxame-core {} (protocolo v{})",
                Enxame::version(),
                enxame_core::PROTOCOL_VERSION
            );
        }
        Command::Init { name } => {
            let mut cfg = load_config(&cli)?;
            if let Some(n) = name {
                cfg.display_name = n.clone();
                save_name(&cfg)?;
            }
            let node = start(cfg).await?;
            print_identity(&node);
            node.shutdown().await;
        }
        Command::Id => {
            let node = start(load_config(&cli)?).await?;
            print_identity(&node);
            println!("endereço:    {:?}", node.endpoint_addr());
            node.shutdown().await;
        }
        Command::Ticket { qr } => {
            let node = start(load_config(&cli)?).await?;
            let ticket = node.invite_ticket().await?;
            if cfg_relay_disabled(&load_config(&cli)?) {
                eprintln!("Aviso: sem relay os endereços do ticket só valem enquanto este processo rodar; use o ticket impresso por `serve`.");
            }
            if *qr {
                println!("{}", ticket.qr_text()?);
            }
            println!("{ticket}");
            eprintln!(
                "\nVálido por ~{}h. Quem tiver o ticket pode pedir para ser seu contato; \
                 você decide ao receber o pedido (`enxame-cli serve`).",
                node_ttl_hours(&load_config(&cli)?)
            );
            node.shutdown().await;
        }
        Command::Add { ticket } => {
            let node = start(load_config(&cli)?).await?;
            eprintln!("Pedido enviado; aguardando o outro lado aceitar (o outro lado precisa estar em `serve`)...");
            let outcome = node.add_contact(ticket).await;
            node.shutdown().await;
            match outcome? {
                AddOutcome::Accepted(c) => {
                    println!("Contato mútuo com {} [{}]", c.display_name, c.fingerprint())
                }
                AddOutcome::AlreadyContact(c) => {
                    println!("{} [{}] já é contato", c.display_name, c.fingerprint())
                }
                AddOutcome::Rejected => bail!("o outro lado recusou o pedido"),
            }
        }
        Command::Serve { auto_accept } => serve(load_config(&cli)?, *auto_accept).await?,
        Command::Contacts => {
            let cfg = load_config(&cli)?;
            let book = enxame_core::list_contacts(&cfg)?;
            if book.is_empty() {
                println!("(nenhum contato)");
            }
            for c in book {
                println!(
                    "{:<20} {}  {}",
                    c.display_name,
                    c.fingerprint(),
                    if c.is_mutual() {
                        "mútuo"
                    } else {
                        "pendente (aguardando o outro lado)"
                    }
                );
            }
        }
        Command::Diag {
            contact,
            wait_direct,
        } => {
            let node = start(load_config(&cli)?).await?;
            eprintln!("Sondando a rede (até ~10s)...");
            let report = node
                .diag(contact.as_deref(), Duration::from_secs(*wait_direct))
                .await;
            node.shutdown().await;
            print_diag(&report?);
        }
    }
    Ok(())
}

fn cfg_relay_disabled(cfg: &Config) -> bool {
    cfg.relay == RelaySetting::Disabled
}

fn node_ttl_hours(cfg: &Config) -> u64 {
    cfg.ticket_ttl_secs / 3600
}

fn save_name(cfg: &Config) -> Result<()> {
    std::fs::create_dir_all(&cfg.data_dir)?;
    let path = cfg.data_dir.join("config.toml");
    let text = toml_with_name(&path, &cfg.display_name)?;
    std::fs::write(&path, text).with_context(|| path.display().to_string())?;
    Ok(())
}

/// Atualiza só `display_name`, preservando o resto do arquivo.
fn toml_with_name(path: &std::path::Path, name: &str) -> Result<String> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    let line = format!("display_name = \"{escaped}\"");
    let mut replaced = false;
    let mut out: Vec<String> = existing
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("display_name") {
                replaced = true;
                line.clone()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !replaced {
        out.insert(0, line);
    }
    Ok(out.join("\n") + "\n")
}

fn print_identity(node: &Enxame) {
    println!("nome:        {}", node.display_name());
    println!("fingerprint: {}", fingerprint(&node.identity_pk()));
    println!("node id:     {}", node.endpoint_id());
}

async fn serve(cfg: Config, auto_accept: bool) -> Result<()> {
    let node = start(cfg).await?;
    print_identity(&node);
    // O ticket deste processo vale enquanto ele rodar: sem relay, os endereços diretos
    // (portas) mudam a cada execução.
    println!(
        "\nTicket (válido enquanto este processo estiver online):\n{}",
        node.invite_ticket().await?
    );
    println!("\nOnline. Ctrl-C para sair.");
    let mut events = node.subscribe();
    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
    let mut lines = {
        use tokio::io::AsyncBufReadExt;
        tokio::io::BufReader::new(tokio::io::stdin()).lines()
    };
    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            ev = events.recv() => match ev {
                Ok(Event::ContactRequested { request_id, display_name, fingerprint }) => {
                    if auto_accept {
                        node.respond_contact(request_id, true)?;
                        println!("Pedido de {display_name} [{fingerprint}] aceito (--auto-accept).");
                    } else {
                        println!(
                            "\nPedido de contato: {display_name} [{fingerprint}]\n\
                             Confirme a fingerprint com a pessoa por outro canal.\n\
                             Aceitar? (s/N): "
                        );
                    }
                }
                Ok(Event::ContactAdded { display_name, fingerprint }) => {
                    println!("Contato mútuo: {display_name} [{fingerprint}]");
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
            line = lines.next_line(), if !auto_accept => {
                let Some(line) = line? else { continue };
                let answer = line.trim().to_lowercase();
                if let Some(req) = node.pending_requests().first() {
                    let accept = matches!(answer.as_str(), "s" | "sim" | "y" | "yes");
                    node.respond_contact(req.id, accept)?;
                    println!("{}", if accept { "Aceito." } else { "Recusado." });
                }
            }
        }
    }
    node.shutdown().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    Ok(())
}

fn print_diag(r: &DiagReport) {
    println!("node id:   {}", r.endpoint_id);
    println!("keystore:  {}", r.keystore);
    println!("papel:     {:?} (fixo até a Fase 3)", r.role);
    println!(
        "NAT:       {:?} (udp: {}, público v4: {}, mapeamento varia por destino: {}, relay preferido: {})",
        r.nat.kind,
        r.nat.udp,
        r.nat.global_v4.map_or("-".into(), |a| a.to_string()),
        r.nat.mapping_varies_by_dest.map_or("?".into(), |b| b.to_string()),
        r.nat.preferred_relay.as_deref().unwrap_or("-"),
    );
    if r.peers.is_empty() {
        println!("(nenhum contato mútuo para sondar)");
    }
    for p in &r.peers {
        println!("\ncontato {} [{}]", p.name, p.fingerprint);
        if let Some(e) = &p.error {
            println!("  inacessível: {e}");
            continue;
        }
        let ms = |d: Option<Duration>| {
            d.map_or("-".into(), |d| {
                format!("{:.1} ms", d.as_secs_f64() * 1000.0)
            })
        };
        println!("  handshake:        {}", ms(p.connect_time));
        println!(
            "  protocolo:        v{}",
            p.protocol_version.map_or("?".into(), |v| v.to_string())
        );
        println!(
            "  papel do par:     {}",
            p.peer_role.map_or("?".into(), |r| format!("{r:?}"))
        );
        if let Some(m) = &p.metrics {
            let kind = match m.kind {
                PathKind::Direct => "direto",
                PathKind::Relay => "relay",
                PathKind::Mixed => "misto (relay selecionado, caminho direto aberto)",
                PathKind::None => "nenhum",
            };
            println!("  caminho:          {kind}");
            println!(
                "  endereço:         {}",
                m.selected_addr.as_deref().unwrap_or("-")
            );
            println!("  RTT:              {}", ms(m.rtt));
            println!("  até ficar direto: {}", ms(m.time_to_direct));
            println!("  trocas de caminho: {}", m.path_switches);
            println!("  caminhos abertos: {}", m.open_paths);
        }
    }
}
