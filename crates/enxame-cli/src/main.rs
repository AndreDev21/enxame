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
    /// Começa em modo jogo: nenhuma contribuição (papel `Leaf`).
    #[arg(long, global = true)]
    game_mode: bool,
    /// Não contribui com a rede dos contatos (papel `Leaf`).
    #[arg(long, global = true)]
    no_contribute: bool,
    /// Papel fixo (`leaf`, `leaf_plus`, `peer`, `relay`, `super_peer`) em vez de automático.
    #[arg(long, global = true)]
    role: Option<String>,
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
    /// Envia uma mensagem a um contato (nome ou prefixo da fingerprint). Se o contato estiver
    /// offline, a mensagem fica guardada e segue na próxima sincronização. Não rode junto com
    /// `serve`/`chat` no mesmo diretório: use `chat`.
    Send { contact: String, text: Vec<String> },
    /// Mostra o histórico da conversa (lê só o banco local).
    History {
        contact: String,
        /// Quantas mensagens mostrar.
        #[arg(short, long, default_value_t = 50)]
        n: usize,
    },
    /// Conversa interativa: fica online, mostra o que chega e envia o que você digita.
    ///
    /// Aceita vários contatos (`chat bob andrecasa`): o texto simples vai para o primeiro e
    /// `@nome texto` escolhe outro destinatário. Mostra o que chegar de qualquer um deles.
    Chat {
        #[arg(required = true)]
        contacts: Vec<String>,
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
    if cli.game_mode {
        cfg.game_mode = true;
    }
    if cli.no_contribute {
        cfg.contribute = false;
    }
    if let Some(r) = &cli.role {
        enxame_core::RoleMode::parse(r)?;
        cfg.role = r.clone();
    }
    Ok(cfg)
}

async fn start(cfg: Config) -> Result<Enxame> {
    let store = open_keystore(&cfg);
    Ok(Enxame::start(cfg, store.as_ref()).await?)
}

fn main() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(run());
    // A leitura de `stdin` do tokio ocupa uma thread bloqueante até alguém apertar Enter; o drop
    // normal do runtime esperaria por ela e o processo não sairia com Ctrl-C. Não esperamos.
    runtime.shutdown_background();
    result
}

async fn run() -> Result<()> {
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
        Command::Send { contact, text } => {
            let node = start(load_config(&cli)?).await?;
            let sent = node.send_text(contact, &text.join(" ")).await;
            node.shutdown().await;
            let sent = sent?;
            if sent.synced {
                println!("Enviada e entregue.");
            } else if sent.carriers > 0 {
                println!(
                    "Guardada; o contato está offline. {} contato(s) em comum vão carregá-la até ele voltar.",
                    sent.carriers
                );
            } else {
                println!(
                    "Guardada; o contato está inacessível agora ({}). Segue na próxima sincronização.",
                    sent.error.as_deref().unwrap_or("?")
                );
            }
        }
        Command::History { contact, n } => {
            let cfg = load_config(&cli)?;
            let node = start(cfg).await?;
            let hist = node.history(contact, *n).await;
            node.shutdown().await;
            let hist = hist?;
            if hist.is_empty() {
                println!("(sem mensagens)");
            }
            for m in hist {
                print_message(&m);
            }
        }
        Command::Chat { contacts } => chat(load_config(&cli)?, contacts).await?,
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

fn print_message(m: &enxame_core::Message) {
    let secs = (m.received_at_ms / 1000) as i64;
    let (h, mi) = ((secs / 3600) % 24, (secs / 60) % 60);
    let who = if m.from_me {
        "eu".to_owned()
    } else {
        m.author_name.clone()
    };
    // ✓ = o destinatário já recebeu (direto ou por recibo de um carrier); … = pendente.
    let mark = match m.delivered {
        Some(true) => " ✓",
        Some(false) => " …",
        None => "",
    };
    println!("[{h:02}:{mi:02} UTC] {who}{mark}: {}", m.text);
}

/// Conversa interativa. Fica online (aceita sessões do contato), mostra mensagens novas assim
/// que chegam e envia cada linha digitada. `/sair` ou Ctrl-C encerram.
async fn chat(cfg: Config, contacts: &[String]) -> Result<()> {
    use tokio::io::AsyncBufReadExt;
    let node = start(cfg).await?;
    // Valida os contatos e mostra o contexto de cada um.
    for contact in contacts {
        for m in node.history(contact, 20).await? {
            print_message(&m);
        }
    }
    println!(
        "--- {} (texto simples vai para {}; `@nome texto` escolhe outro; /sair para sair) ---",
        contacts.join(", "),
        contacts[0]
    );
    let mut events = node.subscribe();
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
    // Sincroniza em segundo plano ao entrar, para puxar o que ficou pendente.
    for contact in contacts {
        let (node, contact) = (node.clone(), contact.clone());
        tokio::spawn(async move {
            let _ = node.sync_now(&contact).await;
        });
    }
    // Contato (da lista) a que uma mensagem recebida pertence.
    let owner = |from_name: &str, from_fp: &str| {
        contacts.iter().find(|c| {
            let q = c.to_lowercase();
            from_name.to_lowercase() == q || (q.len() >= 4 && from_fp.starts_with(&q))
        })
    };
    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            ev = events.recv() => match ev {
                Ok(Event::MessageReceived { from_name, from_fingerprint, .. }) => {
                    // Só as conversas abertas; as outras ficam no histórico delas.
                    if let Some(contact) = owner(&from_name, &from_fingerprint) {
                        if let Some(m) = node.history(contact, 1).await?.pop() {
                            print_message(&m);
                        }
                    }
                }
                Ok(Event::ContactRequested { display_name, fingerprint, .. }) => {
                    println!("(pedido de contato de {display_name} [{fingerprint}]; use `serve` para responder)");
                }
                Ok(Event::RoleChanged { from, to }) => println!("(papel: {from:?} → {to:?})"),
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let line = line.trim();
                if line == "/sair" { break; }
                if line.is_empty() { continue; }
                if line.starts_with('/') {
                    chat_command(&node, line).await;
                    continue;
                }
                // `@nome` sem texto é um engano: não manda "@nome" como mensagem.
                if line.starts_with('@') && !line.contains(' ') {
                    println!("(uso: @nome texto)");
                    continue;
                }
                // `@nome texto` escolhe o destinatário; senão, o primeiro contato.
                let (target, text) = match line.strip_prefix('@').and_then(|r| r.split_once(' ')) {
                    Some((name, text)) => (name.to_owned(), text.trim().to_owned()),
                    None => (contacts[0].clone(), line.to_owned()),
                };
                match node.send_text(&target, &text).await {
                    Ok(s) if s.synced => println!("(→ {target}: entregue)"),
                    Ok(s) if s.carriers > 0 => {
                        println!("(→ {target}: offline; {} carrier(s) vão entregar quando ele voltar)", s.carriers)
                    }
                    Ok(_) => println!("(→ {target}: guardada; inacessível agora, segue depois)"),
                    Err(e) => println!("erro: {e}"),
                }
            }
        }
    }
    node.shutdown().await;
    Ok(())
}

/// Comandos do chat: `/papel`, `/jogo on|off`, `/contribuir on|off`, `/contribuicao`.
async fn chat_command(node: &Enxame, line: &str) {
    let mut parts = line.split_whitespace();
    let on = |a: Option<&str>| matches!(a, Some("on" | "sim" | "1"));
    match parts.next() {
        Some("/papel") => println!("(papel atual: {:?})", node.role()),
        Some("/jogo") => {
            node.set_game_mode(on(parts.next())).await;
            println!("(modo jogo; papel atual: {:?})", node.role());
        }
        Some("/contribuir") => {
            node.set_contribution(on(parts.next())).await;
            println!("(papel atual: {:?})", node.role());
        }
        Some("/contribuicao") => match node.contribution_summary().await {
            Ok(s) => println!(
                "(mês {}: {} bytes servidos de {} permitidos; guardando {} mensagens de terceiros)",
                s.month, s.bytes_served, s.budget_bytes, s.carry_items
            ),
            Err(e) => println!("erro: {e}"),
        },
        _ => println!("(comandos: /papel, /jogo on|off, /contribuir on|off, /contribuicao, /sair)"),
    }
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
                Ok(Event::MessageReceived { from_name, .. }) => {
                    if let Some(m) = node.history(&from_name, 1).await.ok().and_then(|mut h| h.pop()) {
                        print_message(&m);
                    }
                }
                Ok(Event::RoleChanged { from, to }) => println!("Papel: {from:?} → {to:?}"),
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
    println!("papel:     {:?}", r.role);
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
