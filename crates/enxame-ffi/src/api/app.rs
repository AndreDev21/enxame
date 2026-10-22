//! Funções que o app Flutter chama. O nó fica num estado global: `start_node` sobe, as demais
//! funções falham com "o nó não está rodando" antes disso. Os contatos são identificados pela
//! fingerprint (única) e os grupos pelo id em hexadecimal.

use std::{path::PathBuf, sync::RwLock, time::Duration};

use data_encoding::HEXLOWER;
use enxame_core::{open_keystore, Config, DnsChoice, Enxame, Event};
use flutter_rust_bridge::frb;

use crate::frb_generated::StreamSink;

static NODE: RwLock<Option<Enxame>> = RwLock::new(None);

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn node() -> Res<Enxame> {
    NODE.read()
        .map_err(|_| "estado interno corrompido".to_owned())?
        .clone()
        .ok_or_else(|| "o nó não está rodando".to_owned())
}

fn hex(id: &[u8; 32]) -> String {
    HEXLOWER.encode(id)
}

// --------------------------------------------------------------------------- tipos

/// Quem eu sou.
pub struct MyIdentity {
    pub name: String,
    pub fingerprint: String,
    pub node_id: String,
    pub role: String,
    /// "sistema", "manual" ou "automático (motivo)".
    pub dns: String,
}

pub struct ContactInfo {
    pub name: String,
    pub fingerprint: String,
    /// Já aceitou (contato mútuo). Falso = pedido enviado, aguardando.
    pub mutual: bool,
}

pub struct ContactRequest {
    pub id: u64,
    pub name: String,
    pub fingerprint: String,
}

pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub mime: String,
}

pub struct MessageInfo {
    pub id: String,
    pub from_me: bool,
    pub author: String,
    pub text: String,
    /// Quando este dispositivo guardou a mensagem (ms desde a época Unix).
    pub time_ms: u64,
    /// Só nas minhas: o destinatário já recebeu. `None` nas recebidas.
    pub delivered: Option<bool>,
    pub file: Option<FileInfo>,
}

pub struct FileItem {
    /// Número para `download_file`.
    pub index: u32,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub from_me: bool,
    /// Já está completo neste dispositivo.
    pub available: bool,
}

pub struct SendResult {
    /// A outra ponta já recebeu.
    pub delivered: bool,
    /// Carriers que guardam a mensagem se a ponta estava offline.
    pub carriers: u32,
}

pub struct GroupMemberInfo {
    pub name: String,
    pub fingerprint: String,
    pub is_me: bool,
    /// Não é meu contato: vê meu IP por causa do grupo.
    pub exposed: bool,
}

pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub creator: String,
    pub i_am_creator: bool,
    /// Falso = fui removido deste grupo.
    pub active: bool,
    pub members: Vec<GroupMemberInfo>,
}

pub struct GroupSendResult {
    pub delivered: u32,
    pub offline: Vec<String>,
    pub carriers: u32,
}

pub struct ContactContribution {
    pub name: String,
    pub bytes: u64,
    pub items: u64,
}

pub struct Contribution {
    pub month: u32,
    pub bytes_served: u64,
    pub budget_bytes: u64,
    pub items_delivered: u64,
    pub carry_items: u64,
    pub carry_bytes: u64,
    pub by_contact: Vec<ContactContribution>,
    pub role: String,
}

pub struct PeerDiagInfo {
    pub name: String,
    pub fingerprint: String,
    pub error: Option<String>,
    /// "direto", "relay", "misto" ou "?".
    pub path: String,
    pub rtt_ms: Option<f64>,
    pub peer_role: Option<String>,
}

pub struct DiagInfo {
    pub node_id: String,
    pub role: String,
    pub dns: String,
    pub nat: String,
    pub relay: Option<String>,
    pub peers: Vec<PeerDiagInfo>,
}

/// Eventos do núcleo, já simplificados para a interface.
pub enum AppEvent {
    /// Chegou mensagem; `conversation_id` é o id do grupo (hex) ou a conversa 1:1.
    MessageReceived {
        conversation_id: String,
        from_name: String,
        from_fingerprint: String,
    },
    ContactRequested {
        request_id: u64,
        name: String,
        fingerprint: String,
    },
    ContactAdded {
        name: String,
        fingerprint: String,
    },
    RoleChanged {
        from: String,
        to: String,
    },
    Transfer {
        name: String,
        done: u64,
        total: u64,
        upload: bool,
    },
    GroupJoined {
        group_id: String,
        name: String,
        creator: String,
        exposed: Vec<String>,
    },
    GroupUpdated {
        group_id: String,
        name: String,
    },
    GroupRemoved {
        group_id: String,
        name: String,
    },
    /// O stream perdeu eventos: a interface deve recarregar o que mostra.
    Lagged,
}

fn dns_text(d: &DnsChoice) -> String {
    match d {
        DnsChoice::System => "sistema".to_owned(),
        DnsChoice::Manual => "manual".to_owned(),
        DnsChoice::Fallback { reason } => format!("automático ({reason})"),
    }
}

// --------------------------------------------------------------------------- ciclo de vida

/// Pasta de dados padrão deste sistema.
#[frb(sync)]
pub fn default_data_dir() -> String {
    enxame_core::default_data_dir().display().to_string()
}

/// Já existe uma identidade nesta pasta (a primeira execução ainda não aconteceu se for falso).
#[frb(sync)]
pub fn has_identity(data_dir: Option<String>) -> bool {
    // `Config::load` já considera `ENXAME_DATA_DIR`; sem config legível, vale o padrão.
    let dir = Config::load(data_dir.map(PathBuf::from))
        .map(|c| c.data_dir)
        .unwrap_or_else(|_| enxame_core::default_data_dir());
    dir.join("identity.key").exists() || dir.join("enxame.db").exists()
}

/// Sobe o nó (gera a identidade na primeira vez). `display_name` só é gravado se informado.
pub async fn start_node(data_dir: Option<String>, display_name: Option<String>) -> Res<MyIdentity> {
    if let Ok(n) = node() {
        return identity_of(&n);
    }
    let mut cfg = Config::load(data_dir.map(PathBuf::from)).map_err(err)?;
    if let Some(name) = display_name
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
    {
        cfg.display_name = name;
        save_name(&cfg).map_err(err)?;
    }
    let keystore = open_keystore(&cfg);
    let node = Enxame::start(cfg, keystore.as_ref()).await.map_err(err)?;
    let me = identity_of(&node)?;
    *NODE
        .write()
        .map_err(|_| "estado interno corrompido".to_owned())? = Some(node);
    Ok(me)
}

/// Para o nó (fecha conexões e libera o banco).
pub async fn stop_node() -> Res<()> {
    let taken = NODE
        .write()
        .map_err(|_| "estado interno corrompido".to_owned())?
        .take();
    if let Some(n) = taken {
        n.shutdown().await;
    }
    Ok(())
}

fn identity_of(n: &Enxame) -> Res<MyIdentity> {
    Ok(MyIdentity {
        name: n.display_name().to_owned(),
        fingerprint: enxame_core::fingerprint(&n.identity_pk()),
        node_id: n.endpoint_id().to_string(),
        role: format!("{:?}", n.role()),
        dns: dns_text(&n.dns_choice()),
    })
}

/// Atualiza só `display_name` em `config.toml`, preservando o resto.
fn save_name(cfg: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(&cfg.data_dir)?;
    let path = cfg.data_dir.join("config.toml");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let escaped = cfg.display_name.replace('\\', "\\\\").replace('"', "\\\"");
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
    std::fs::write(path, out.join("\n") + "\n")
}

pub fn my_identity() -> Res<MyIdentity> {
    identity_of(&node()?)
}

// --------------------------------------------------------------------------- contatos

/// Meu ticket de convite (`enxame1...`), para copiar ou virar QR.
pub async fn my_ticket() -> Res<String> {
    Ok(node()?.invite_ticket().await.map_err(err)?.to_string())
}

/// Importa o ticket de outra pessoa e espera ela aceitar. Devolve "aceito", "recusado" ou
/// "já é contato".
pub async fn add_contact(ticket: String) -> Res<String> {
    use enxame_core::AddOutcome;
    Ok(
        match node()?.add_contact(ticket.trim()).await.map_err(err)? {
            AddOutcome::Accepted(_) => "aceito",
            AddOutcome::Rejected => "recusado",
            AddOutcome::AlreadyContact(_) => "já é contato",
        }
        .to_owned(),
    )
}

pub fn contacts() -> Res<Vec<ContactInfo>> {
    Ok(node()?
        .contacts()
        .into_iter()
        .map(|c| ContactInfo {
            name: c.display_name.clone(),
            fingerprint: c.fingerprint(),
            mutual: c.is_mutual(),
        })
        .collect())
}

pub fn pending_requests() -> Res<Vec<ContactRequest>> {
    Ok(node()?
        .pending_requests()
        .into_iter()
        .map(|r| ContactRequest {
            id: r.id,
            name: r.display_name,
            fingerprint: r.fingerprint,
        })
        .collect())
}

pub fn respond_request(id: u64, accept: bool) -> Res<()> {
    node()?.respond_contact(id, accept).map_err(err)
}

// --------------------------------------------------------------------------- mensagens 1:1

fn msg_of(m: enxame_core::Message) -> MessageInfo {
    MessageInfo {
        id: hex(&m.id),
        from_me: m.from_me,
        author: m.author_name,
        text: m.text,
        time_ms: m.received_at_ms,
        delivered: m.delivered,
        file: m.file.map(|f| FileInfo {
            name: f.name,
            size: f.size,
            mime: f.mime,
        }),
    }
}

pub async fn history(contact: String, limit: u32) -> Res<Vec<MessageInfo>> {
    let msgs = node()?
        .history(&contact, limit as usize)
        .await
        .map_err(err)?;
    Ok(msgs.into_iter().map(msg_of).collect())
}

pub async fn send_text(contact: String, text: String) -> Res<SendResult> {
    let s = node()?.send_text(&contact, &text).await.map_err(err)?;
    Ok(SendResult {
        delivered: s.synced,
        carriers: s.carriers as u32,
    })
}

// --------------------------------------------------------------------------- grupos

fn group_of(g: enxame_core::GroupInfo) -> GroupInfo {
    GroupInfo {
        id: hex(&g.id),
        name: g.name,
        creator: g.creator_name,
        i_am_creator: g.i_am_creator,
        active: g.active,
        members: g
            .members
            .into_iter()
            .map(|m| GroupMemberInfo {
                name: m.name,
                fingerprint: m.fingerprint,
                is_me: m.is_me,
                exposed: !m.is_me && !m.is_contact,
            })
            .collect(),
    }
}

pub async fn groups() -> Res<Vec<GroupInfo>> {
    Ok(node()?
        .groups()
        .await
        .map_err(err)?
        .into_iter()
        .map(group_of)
        .collect())
}

/// Cria um grupo com contatos mútuos (fingerprints).
pub async fn create_group(name: String, members: Vec<String>) -> Res<GroupInfo> {
    let refs: Vec<&str> = members.iter().map(String::as_str).collect();
    let (g, _) = node()?.create_group(&name, &refs).await.map_err(err)?;
    Ok(group_of(g))
}

pub async fn add_group_member(group: String, contact: String) -> Res<GroupInfo> {
    let (g, _) = node()?
        .add_group_member(&group, &contact)
        .await
        .map_err(err)?;
    Ok(group_of(g))
}

pub async fn remove_group_member(group: String, member: String) -> Res<GroupInfo> {
    let (g, _) = node()?
        .remove_group_member(&group, &member)
        .await
        .map_err(err)?;
    Ok(group_of(g))
}

pub async fn rename_group(group: String, new_name: String) -> Res<GroupInfo> {
    let (g, _) = node()?.rename_group(&group, &new_name).await.map_err(err)?;
    Ok(group_of(g))
}

pub async fn group_history(group: String, limit: u32) -> Res<Vec<MessageInfo>> {
    let msgs = node()?
        .group_history(&group, limit as usize)
        .await
        .map_err(err)?;
    Ok(msgs.into_iter().map(msg_of).collect())
}

pub async fn send_group_text(group: String, text: String) -> Res<GroupSendResult> {
    let s = node()?.send_group_text(&group, &text).await.map_err(err)?;
    Ok(GroupSendResult {
        delivered: s.delivered as u32,
        offline: s.offline,
        carriers: s.carriers as u32,
    })
}

// --------------------------------------------------------------------------- arquivos

pub async fn files(contact: String) -> Res<Vec<FileItem>> {
    Ok(node()?
        .files(&contact)
        .await
        .map_err(err)?
        .into_iter()
        .map(|f| FileItem {
            index: f.index as u32,
            name: f.name,
            size: f.size,
            mime: f.mime,
            from_me: f.from_me,
            available: f.available,
        })
        .collect())
}

/// Envia o mesmo arquivo (cifrado uma vez) a um ou mais contatos.
pub async fn send_file(contacts: Vec<String>, path: String) -> Res<()> {
    let refs: Vec<&str> = contacts.iter().map(String::as_str).collect();
    node()?
        .send_file_to(&refs, std::path::Path::new(&path))
        .await
        .map_err(err)?;
    Ok(())
}

/// Baixa o arquivo `index` (de `files`). Devolve o caminho salvo.
pub async fn download_file(contact: String, index: u32) -> Res<String> {
    let d = node()?
        .download_file(&contact, &index.to_string(), None)
        .await
        .map_err(err)?;
    Ok(d.path.display().to_string())
}

// --------------------------------------------------------------------------- contribuição e rede

pub fn role() -> Res<String> {
    Ok(format!("{:?}", node()?.role()))
}

pub async fn set_game_mode(on: bool) -> Res<String> {
    let n = node()?;
    n.set_game_mode(on).await;
    Ok(format!("{:?}", n.role()))
}

pub async fn set_contribution(on: bool) -> Res<String> {
    let n = node()?;
    n.set_contribution(on).await;
    Ok(format!("{:?}", n.role()))
}

pub async fn contribution() -> Res<Contribution> {
    let n = node()?;
    let s = n.contribution_summary().await.map_err(err)?;
    Ok(Contribution {
        month: s.month,
        bytes_served: s.bytes_served,
        budget_bytes: s.budget_bytes,
        items_delivered: s.items_delivered,
        carry_items: s.carry_items,
        carry_bytes: s.carry_bytes,
        by_contact: s
            .by_contact
            .into_iter()
            .map(|(name, bytes, items)| ContactContribution { name, bytes, items })
            .collect(),
        role: format!("{:?}", n.role()),
    })
}

/// Diagnóstico de rede (NAT, DNS, caminho e RTT por contato). Pode levar ~10 s.
pub async fn diag() -> Res<DiagInfo> {
    use enxame_core::PathKind;
    let r = node()?
        .diag(None, Duration::from_secs(5))
        .await
        .map_err(err)?;
    Ok(DiagInfo {
        node_id: r.endpoint_id.to_string(),
        role: format!("{:?}", r.role),
        dns: dns_text(&r.dns),
        nat: format!("{:?}", r.nat.kind),
        relay: r.nat.preferred_relay,
        peers: r
            .peers
            .into_iter()
            .map(|p| {
                let path = match p.metrics.as_ref().map(|m| m.kind) {
                    Some(PathKind::Direct) => "direto",
                    Some(PathKind::Relay) => "relay",
                    Some(PathKind::Mixed) => "misto",
                    _ => "?",
                };
                PeerDiagInfo {
                    name: p.name,
                    fingerprint: p.fingerprint,
                    error: p.error,
                    path: path.to_owned(),
                    rtt_ms: p
                        .metrics
                        .and_then(|m| m.rtt)
                        .map(|d| d.as_secs_f64() * 1000.0),
                    peer_role: p.peer_role.map(|r| format!("{r:?}")),
                }
            })
            .collect(),
    })
}

// --------------------------------------------------------------------------- eventos

/// Fluxo de eventos do núcleo. Termina quando o nó para ou o Dart cancela a assinatura.
pub async fn events(sink: StreamSink<AppEvent>) -> Res<()> {
    let mut rx = node()?.subscribe();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                if sink.add(AppEvent::Lagged).is_err() {
                    return Ok(());
                }
                continue;
            }
            Err(_) => return Ok(()),
        };
        let mapped = match ev {
            Event::MessageReceived {
                conversation_id,
                from_name,
                from_fingerprint,
                ..
            } => Some(AppEvent::MessageReceived {
                conversation_id: hex(&conversation_id),
                from_name,
                from_fingerprint,
            }),
            Event::ContactRequested {
                request_id,
                display_name,
                fingerprint,
            } => Some(AppEvent::ContactRequested {
                request_id,
                name: display_name,
                fingerprint,
            }),
            Event::ContactAdded {
                display_name,
                fingerprint,
            } => Some(AppEvent::ContactAdded {
                name: display_name,
                fingerprint,
            }),
            Event::RoleChanged { from, to } => Some(AppEvent::RoleChanged {
                from: format!("{from:?}"),
                to: format!("{to:?}"),
            }),
            Event::TransferProgress {
                name,
                done,
                total,
                upload,
                ..
            } => Some(AppEvent::Transfer {
                name,
                done,
                total,
                upload,
            }),
            Event::GroupJoined {
                group_id,
                name,
                creator_name,
                exposed,
            } => Some(AppEvent::GroupJoined {
                group_id: hex(&group_id),
                name,
                creator: creator_name,
                exposed,
            }),
            Event::GroupUpdated { group_id, name, .. } => Some(AppEvent::GroupUpdated {
                group_id: hex(&group_id),
                name,
            }),
            Event::GroupRemoved { group_id, name } => Some(AppEvent::GroupRemoved {
                group_id: hex(&group_id),
                name,
            }),
            _ => None,
        };
        if let Some(m) = mapped {
            if sink.add(m).is_err() {
                return Ok(());
            }
        }
    }
}
