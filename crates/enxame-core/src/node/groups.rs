//! Grupos pequenos no nó: criar, mudar a lista, enviar e acompanhar (ADR 0009).

use std::collections::HashMap;

use super::{
    build_history_conv, carry_flow, files::futures_join_all, send_to_contact, sync_with, Enxame,
    Inner, Message,
};
use crate::{
    contacts::Contact,
    crypto::{Envelope, EnvelopeKind, Recipient, MAX_TEXT_BYTES},
    groups::{GroupMember, GroupOpKind, GroupSnapshot, StoredGroup, MAX_GROUP_MEMBERS},
    identity::{unix_now, unix_now_ms},
    store, Error, Event, Result,
};

/// Um membro como a UI o mostra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMemberInfo {
    pub name: String,
    pub fingerprint: String,
    pub is_me: bool,
    /// Já é meu contato mútuo. Quem não é conhece meu IP por causa do grupo.
    pub is_contact: bool,
}

/// Resumo de um grupo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupInfo {
    pub id: [u8; 32],
    pub name: String,
    pub creator_name: String,
    pub i_am_creator: bool,
    /// Ainda sou membro no retrato mais novo que conheço.
    pub active: bool,
    pub seq: u64,
    pub members: Vec<GroupMemberInfo>,
}

/// Resultado de enviar uma mensagem ao grupo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentGroupMessage {
    pub id: [u8; 32],
    /// Membros que já receberam (sincronização direta bem-sucedida).
    pub delivered: usize,
    /// Nomes dos membros inacessíveis agora; a mensagem fica pendente para eles.
    pub offline: Vec<String>,
    /// Carriers distintos que passaram a guardar cópia para os membros offline.
    pub carriers: usize,
}

fn info_of(inner: &Inner, g: &StoredGroup) -> GroupInfo {
    let me = inner.identity.identity_pk();
    let contacts = inner.mutual_contacts();
    let snap = &g.snapshot;
    GroupInfo {
        id: g.id(),
        name: snap.name.clone(),
        creator_name: snap
            .member(&snap.creator)
            .map(|m| m.display_name.clone())
            .unwrap_or_default(),
        i_am_creator: snap.creator == me,
        active: g.active,
        seq: snap.seq,
        members: snap
            .members
            .iter()
            .map(|m| GroupMemberInfo {
                name: m.display_name.clone(),
                fingerprint: m.fingerprint(),
                is_me: m.identity_pk == me,
                is_contact: contacts.iter().any(|c| c.identity_pk == m.identity_pk),
            })
            .collect(),
    }
}

/// Acha um grupo por nome (sem diferenciar caixa; aceita o `#` na frente) ou por prefixo do id.
fn resolve_group<'a>(groups: &'a [StoredGroup], query: &str) -> Result<&'a StoredGroup> {
    let q = query.trim().trim_start_matches('#').to_lowercase();
    let hits: Vec<&StoredGroup> = groups
        .iter()
        .filter(|g| {
            g.snapshot.name.to_lowercase() == q || (q.len() >= 4 && hex(&g.id()).starts_with(&q))
        })
        .collect();
    // Se há um grupo ativo e outros inativos com o mesmo nome, vale o ativo.
    let active: Vec<&&StoredGroup> = hits.iter().filter(|g| g.active).collect();
    match (hits.as_slice(), active.as_slice()) {
        ([one], _) => Ok(one),
        (_, [one]) => Ok(**one),
        ([], _) => Err(Error::Contact(format!("nenhum grupo `{query}`"))),
        _ => Err(Error::Contact(format!("`{query}` é ambíguo"))),
    }
}

fn hex(id: &[u8; 32]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

fn self_member(inner: &Inner, addr: iroh::EndpointAddr) -> GroupMember {
    GroupMember {
        identity_pk: inner.identity.identity_pk(),
        enc_pk: inner.identity.enc_pk(),
        device_cert: inner.identity.device_cert(unix_now()),
        node_addr: addr,
        display_name: inner.cfg.display_name.clone(),
    }
}

/// Quem vou discar para falar com `m`: o contato (endereço mais fresco) ou o dado do retrato.
fn peer_for(inner: &Inner, m: &GroupMember) -> Contact {
    inner
        .book()
        .by_identity(&m.identity_pk)
        .filter(|c| c.is_mutual())
        .cloned()
        .unwrap_or_else(|| m.as_peer())
}

/// Entrega o retrato por mensagem 1:1 a cada alvo (o criador só tem contatos como membros).
/// Devolve em quantos a entrega foi direta.
async fn dispatch_snapshot(inner: &Inner, snap: &GroupSnapshot, targets: &[Contact]) -> usize {
    let Ok(plain) = snap.encode() else { return 0 };
    let sends = targets
        .iter()
        .map(|t| send_to_contact(inner, t, EnvelopeKind::GroupOp, plain.clone()));
    futures_join_all(sends)
        .await
        .into_iter()
        .filter(|r| r.as_ref().is_ok_and(|m| m.synced))
        .count()
}

/// Compara o estado dos grupos com `before` e emite os eventos; também ensina ao endpoint como
/// alcançar os co-membros (o downloader de blobs conecta só por `EndpointId`).
pub(super) async fn emit_group_changes(inner: &Inner, before: &[StoredGroup]) {
    let Ok(after) = inner.groups_now().await else {
        return;
    };
    let me = inner.identity.identity_pk();
    let contacts = inner.mutual_contacts();
    for a in &after {
        let old = before.iter().find(|b| b.id() == a.id());
        let event = match old {
            None if a.active => Some(joined(a, &me, &contacts)),
            Some(b) if a.snapshot.seq > b.snapshot.seq => {
                if b.active && !a.active {
                    Some(Event::GroupRemoved {
                        group_id: a.id(),
                        name: a.snapshot.name.clone(),
                    })
                } else if !b.active && a.active {
                    Some(joined(a, &me, &contacts))
                } else {
                    Some(Event::GroupUpdated {
                        group_id: a.id(),
                        name: a.snapshot.name.clone(),
                        seq: a.snapshot.seq,
                    })
                }
            }
            _ => None,
        };
        if let Some(ev) = event {
            inner.emit(ev);
        }
    }
    if let Ok(co) = inner.co_members().await {
        for c in &co {
            inner.register_addr(c);
        }
    }
}

fn joined(g: &StoredGroup, me: &[u8; 32], contacts: &[Contact]) -> Event {
    let snap = &g.snapshot;
    Event::GroupJoined {
        group_id: g.id(),
        name: snap.name.clone(),
        creator_name: snap
            .member(&snap.creator)
            .map(|m| m.display_name.clone())
            .unwrap_or_default(),
        exposed: snap
            .members
            .iter()
            .filter(|m| {
                &m.identity_pk != me && !contacts.iter().any(|c| c.identity_pk == m.identity_pk)
            })
            .map(|m| format!("{} [{}]", m.display_name, m.fingerprint()))
            .collect(),
    }
}

impl Inner {
    /// Contato mútuo para cada membro de `snap` (o criador só agrega contatos).
    fn contacts_of(&self, snap: &GroupSnapshot, extra: &[Contact]) -> Vec<Contact> {
        let me = self.identity.identity_pk();
        let mut out: Vec<Contact> = Vec::new();
        for m in &snap.members {
            if m.identity_pk != me {
                if let Some(c) = self.book().by_identity(&m.identity_pk).cloned() {
                    out.push(c);
                }
            }
        }
        for c in extra {
            if !out.iter().any(|o| o.identity_pk == c.identity_pk) {
                out.push(c.clone());
            }
        }
        out
    }

    async fn creator_group(&self, query: &str) -> Result<StoredGroup> {
        let groups = self.groups_now().await?;
        let g = resolve_group(&groups, query)?.clone();
        if !g.active || g.snapshot.creator != self.identity.identity_pk() {
            return Err(Error::Contact(
                "só o criador do grupo pode mudar a lista de membros".into(),
            ));
        }
        Ok(g)
    }

    async fn publish(
        &self,
        snap: &GroupSnapshot,
        targets: &[Contact],
    ) -> Result<(GroupInfo, usize)> {
        let me = self.identity.identity_pk();
        let snap2 = snap.clone();
        self.store
            .run(move |c| store::group_put(c, &snap2, &me, unix_now_ms()))
            .await?;
        let delivered = dispatch_snapshot(self, snap, targets).await;
        let stored = StoredGroup {
            snapshot: snap.clone(),
            active: true,
            updated_at: unix_now_ms(),
        };
        Ok((info_of(self, &stored), delivered))
    }
}

impl Enxame {
    /// Cria um grupo com `members` (contatos mútuos meus; no máximo 19, além de mim) e entrega o
    /// retrato a cada um por mensagem 1:1. Devolve o grupo e em quantos a entrega foi direta.
    pub async fn create_group(&self, name: &str, members: &[&str]) -> Result<(GroupInfo, usize)> {
        let inner = &*self.inner;
        let me = inner.identity.identity_pk();
        let mut contacts: Vec<Contact> = Vec::new();
        for q in members {
            let c = inner.book().resolve(q)?.clone();
            if !c.is_mutual() {
                return Err(Error::Contact(format!(
                    "`{}` ainda não aceitou (não é mútuo)",
                    c.display_name
                )));
            }
            if !contacts.iter().any(|x| x.identity_pk == c.identity_pk) {
                contacts.push(c);
            }
        }
        if contacts.is_empty() {
            return Err(Error::Contact(
                "um grupo precisa de ao menos um contato".into(),
            ));
        }
        if contacts.len() + 1 > MAX_GROUP_MEMBERS {
            return Err(Error::Contact(format!(
                "grupos têm no máximo {MAX_GROUP_MEMBERS} membros"
            )));
        }
        let mut list = vec![self_member(inner, self.endpoint_addr())];
        list.extend(contacts.iter().map(GroupMember::from_contact));
        let snap = GroupSnapshot::create(me, name, list)?;
        inner.publish(&snap, &contacts).await
    }

    /// Adiciona um contato meu ao grupo (só o criador).
    pub async fn add_group_member(&self, group: &str, contact: &str) -> Result<(GroupInfo, usize)> {
        let inner = &*self.inner;
        let g = inner.creator_group(group).await?;
        let c = inner.book().resolve(contact)?.clone();
        if !c.is_mutual() {
            return Err(Error::Contact(
                "o contato ainda não aceitou (não é mútuo)".into(),
            ));
        }
        if g.snapshot.has(&c.identity_pk) {
            return Err(Error::Contact(format!(
                "`{}` já está no grupo",
                c.display_name
            )));
        }
        let mut members = g.snapshot.members.clone();
        // Atualiza o meu próprio endereço no retrato.
        if let Some(m) = members
            .iter_mut()
            .find(|m| m.identity_pk == inner.identity.identity_pk())
        {
            *m = self_member(inner, self.endpoint_addr());
        }
        members.push(GroupMember::from_contact(&c));
        let snap = g.snapshot.next(
            GroupOpKind::Add {
                member: c.identity_pk,
            },
            g.snapshot.name.clone(),
            members,
        )?;
        let targets = inner.contacts_of(&snap, &[]);
        inner.publish(&snap, &targets).await
    }

    /// Remove um membro (só o criador). O removido recebe o retrato novo, fica sabendo que saiu
    /// e deixa de ser destinatário das mensagens seguintes.
    pub async fn remove_group_member(
        &self,
        group: &str,
        member: &str,
    ) -> Result<(GroupInfo, usize)> {
        let inner = &*self.inner;
        let g = inner.creator_group(group).await?;
        let q = member.trim().to_lowercase();
        let hits: Vec<&GroupMember> = g
            .snapshot
            .members
            .iter()
            .filter(|m| {
                m.display_name.to_lowercase() == q
                    || (q.len() >= 4 && m.fingerprint().starts_with(&q))
            })
            .collect();
        let target = match hits.as_slice() {
            [one] => (*one).clone(),
            [] => return Err(Error::Contact(format!("nenhum membro `{member}`"))),
            _ => return Err(Error::Contact(format!("`{member}` é ambíguo"))),
        };
        if target.identity_pk == g.snapshot.creator {
            return Err(Error::Contact("o criador não pode ser removido".into()));
        }
        let members: Vec<GroupMember> = g
            .snapshot
            .members
            .iter()
            .filter(|m| m.identity_pk != target.identity_pk)
            .cloned()
            .collect();
        let snap = g.snapshot.next(
            GroupOpKind::Remove {
                member: target.identity_pk,
            },
            g.snapshot.name.clone(),
            members,
        )?;
        let removed = inner.book().by_identity(&target.identity_pk).cloned();
        let extra: Vec<Contact> = removed.into_iter().collect();
        let targets = inner.contacts_of(&snap, &extra);
        inner.publish(&snap, &targets).await
    }

    /// Renomeia o grupo (só o criador).
    pub async fn rename_group(&self, group: &str, new_name: &str) -> Result<(GroupInfo, usize)> {
        let inner = &*self.inner;
        let g = inner.creator_group(group).await?;
        let snap = g.snapshot.next(
            GroupOpKind::Rename,
            new_name.to_owned(),
            g.snapshot.members.clone(),
        )?;
        let targets = inner.contacts_of(&snap, &[]);
        inner.publish(&snap, &targets).await
    }

    /// Todos os grupos que conheço (inclusive aqueles de que fui removido).
    pub async fn groups(&self) -> Result<Vec<GroupInfo>> {
        let groups = self.inner.groups_now().await?;
        Ok(groups.iter().map(|g| info_of(&self.inner, g)).collect())
    }

    /// Envia uma mensagem de texto ao grupo: um envelope selado para todos os membros. Entrega
    /// direta a quem estiver online; para os offline pede carriers (membros e contatos em comum).
    pub async fn send_group_text(&self, group: &str, text: &str) -> Result<SentGroupMessage> {
        if text.trim().is_empty() {
            return Err(Error::Contact("mensagem vazia".into()));
        }
        if text.len() > MAX_TEXT_BYTES {
            return Err(Error::Contact(format!(
                "mensagem acima de {MAX_TEXT_BYTES} bytes"
            )));
        }
        let inner = &*self.inner;
        let groups = inner.groups_now().await?;
        let g = resolve_group(&groups, group)?.clone();
        if !g.active {
            return Err(Error::Contact("você não está mais neste grupo".into()));
        }
        let me = inner.identity.identity_pk();
        let conv = g.id();
        let parents = inner.store.run(move |db| store::heads(db, &conv)).await?;
        let hlc = inner.clock().tick(unix_now_ms());
        let recipients: Vec<Recipient> = g
            .snapshot
            .members
            .iter()
            .map(|m| Recipient {
                identity_pk: m.identity_pk,
                enc_pk: m.enc_pk,
            })
            .collect();
        let plain = text.as_bytes().to_vec();
        let env = Envelope::create(
            &inner.identity,
            conv,
            hlc,
            parents,
            &recipients,
            EnvelopeKind::Text,
            &plain,
        )?;
        let id = env.id;
        let others: Vec<GroupMember> = g
            .snapshot
            .members
            .iter()
            .filter(|m| m.identity_pk != me)
            .cloned()
            .collect();
        let pks: Vec<[u8; 32]> = others.iter().map(|m| m.identity_pk).collect();
        inner
            .store
            .run(move |db| {
                store::envelope_insert(db, &env, Some(&plain), true, unix_now_ms())?;
                for pk in &pks {
                    store::pending_add(db, &id, pk, unix_now_ms())?;
                }
                Ok(())
            })
            .await?;
        // Entrega direta a cada membro; quem falhar ganha carriers.
        let tries = others.iter().map(|m| async move {
            let peer = peer_for(inner, m);
            match sync_with(inner, &peer).await {
                Ok(_) => (m.display_name.clone(), 0usize, true),
                Err(e) => {
                    tracing::debug!(peer = %m.fingerprint(), "membro inacessível: {e}");
                    let n = carry_flow::offer_to_carriers(inner, &peer)
                        .await
                        .unwrap_or(0);
                    (m.display_name.clone(), n, false)
                }
            }
        });
        let results = futures_join_all(tries).await;
        let delivered = results.iter().filter(|r| r.2).count();
        let offline: Vec<String> = results
            .iter()
            .filter(|r| !r.2)
            .map(|r| r.0.clone())
            .collect();
        let carriers = results.iter().map(|r| r.1).max().unwrap_or(0);
        Ok(SentGroupMessage {
            id,
            delivered,
            offline,
            carriers,
        })
    }

    /// Sincroniza o grupo com todos os outros membros agora. Devolve quantos responderam.
    pub async fn sync_group(&self, group: &str) -> Result<usize> {
        let inner = &*self.inner;
        let groups = inner.groups_now().await?;
        let g = resolve_group(&groups, group)?.clone();
        let me = inner.identity.identity_pk();
        let tries = g
            .snapshot
            .members
            .iter()
            .filter(|m| m.identity_pk != me)
            .map(|m| async move { sync_with(inner, &peer_for(inner, m)).await.is_ok() });
        Ok(futures_join_all(tries)
            .await
            .into_iter()
            .filter(|ok| *ok)
            .count())
    }

    /// Últimas `limit` mensagens do grupo, na ordem de exibição.
    pub async fn group_history(&self, group: &str, limit: usize) -> Result<Vec<Message>> {
        let inner = &*self.inner;
        let groups = inner.groups_now().await?;
        let g = resolve_group(&groups, group)?.clone();
        let me = inner.identity.identity_pk();
        let mut names: HashMap<[u8; 32], String> = g
            .snapshot
            .members
            .iter()
            .map(|m| (m.identity_pk, m.display_name.clone()))
            .collect();
        names.insert(me, inner.cfg.display_name.clone());
        let owners: Vec<[u8; 32]> = g
            .snapshot
            .members
            .iter()
            .map(|m| m.identity_pk)
            .filter(|pk| *pk != me)
            .collect();
        build_history_conv(&inner.store, &inner.cfg, g.id(), &owners, &names, limit).await
    }
}
