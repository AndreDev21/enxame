//! Grupos pequenos (spec 7.2, ADR 0009): retratos assinados pelo criador e consulta de membros.
//!
//! Um grupo é uma sequência de [`GroupSnapshot`]s numerados por `seq`; vale o de maior `seq`.
//! O retrato viaja num envelope `GroupOp` da conversa 1:1 entre o criador e cada membro.

use std::collections::HashSet;

use iroh::EndpointAddr;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::{
    contacts::{Contact, ContactStatus},
    identity::DeviceCert,
    Error, Result,
};

/// Versão do formato do retrato.
pub const GROUP_VERSION: u8 = 1;
/// Máximo de membros, contando o criador (spec 7.2).
pub const MAX_GROUP_MEMBERS: usize = 20;
/// Máximo de caracteres do nome do grupo e dos nomes de exibição.
pub const MAX_GROUP_NAME: usize = 64;
/// Limite do retrato serializado, para recusar lixo antes de decodificar.
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024;
const DOMAIN_GROUP: &[u8] = b"enxame/group/1\0";

/// Identificador do grupo: só o criador consegue produzir o par `(criador, nonce)`.
pub fn group_id(creator: &[u8; 32], nonce: &[u8; 16]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(DOMAIN_GROUP);
    h.update(creator);
    h.update(nonce);
    *h.finalize().as_bytes()
}

/// Um membro: o necessário para selar mensagens para ele e para discá-lo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    pub identity_pk: [u8; 32],
    pub enc_pk: [u8; 32],
    pub device_cert: DeviceCert,
    pub node_addr: EndpointAddr,
    pub display_name: String,
}

impl GroupMember {
    pub fn from_contact(c: &Contact) -> Self {
        Self {
            identity_pk: c.identity_pk,
            enc_pk: c.enc_pk,
            device_cert: c.device_cert.clone(),
            node_addr: c.node_addr.clone(),
            display_name: c.display_name.clone(),
        }
    }

    /// Visão de par de rede (nunca persistida como contato): vale como `Mutual` só dentro do
    /// núcleo, para a sincronização e o carry tratarem co-membros como pares.
    pub fn as_peer(&self) -> Contact {
        Contact {
            identity_pk: self.identity_pk,
            enc_pk: self.enc_pk,
            device_cert: self.device_cert.clone(),
            node_addr: self.node_addr.clone(),
            display_name: self.display_name.clone(),
            status: ContactStatus::Mutual,
            added_at: 0,
        }
    }

    pub fn fingerprint(&self) -> String {
        crate::identity::fingerprint(&self.identity_pk)
    }
}

/// O que o retrato mudou (informativo: o estado é a lista completa).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupOpKind {
    Create,
    Add { member: [u8; 32] },
    Remove { member: [u8; 32] },
    Rename,
}

/// Estado completo do grupo após uma operação do criador.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSnapshot {
    pub version: u8,
    pub nonce: [u8; 16],
    pub creator: [u8; 32],
    pub seq: u64,
    pub name: String,
    pub members: Vec<GroupMember>,
    pub op: GroupOpKind,
}

impl GroupSnapshot {
    /// Primeiro retrato de um grupo novo.
    pub fn create(creator: [u8; 32], name: &str, members: Vec<GroupMember>) -> Result<Self> {
        let mut nonce = [0u8; 16];
        OsRng.fill_bytes(&mut nonce);
        let snap = Self {
            version: GROUP_VERSION,
            nonce,
            creator,
            seq: 1,
            name: name.trim().to_owned(),
            members,
            op: GroupOpKind::Create,
        };
        snap.validate()?;
        Ok(snap)
    }

    pub fn id(&self) -> [u8; 32] {
        group_id(&self.creator, &self.nonce)
    }

    pub fn member(&self, pk: &[u8; 32]) -> Option<&GroupMember> {
        self.members.iter().find(|m| &m.identity_pk == pk)
    }

    pub fn has(&self, pk: &[u8; 32]) -> bool {
        self.member(pk).is_some()
    }

    /// Próximo retrato (mesmo grupo, `seq + 1`) com outra lista/nome.
    pub fn next(&self, op: GroupOpKind, name: String, members: Vec<GroupMember>) -> Result<Self> {
        let snap = Self {
            version: GROUP_VERSION,
            nonce: self.nonce,
            creator: self.creator,
            seq: self.seq + 1,
            name: name.trim().to_owned(),
            members,
            op,
        };
        snap.validate()?;
        Ok(snap)
    }

    /// Estrutura, limites e certificados. Não decide se o autor pode emitir este retrato.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: &str| Err(Error::Protocol(format!("retrato de grupo inválido: {m}")));
        if self.version != GROUP_VERSION {
            return bad("versão não suportada");
        }
        if self.seq == 0 {
            return bad("seq zero");
        }
        if self.name.is_empty() || self.name.chars().count() > MAX_GROUP_NAME {
            return bad("nome vazio ou longo demais");
        }
        if self.members.is_empty() || self.members.len() > MAX_GROUP_MEMBERS {
            return bad("número de membros fora do limite");
        }
        let mut seen = HashSet::new();
        for m in &self.members {
            if !seen.insert(m.identity_pk) {
                return bad("membro repetido");
            }
            if m.display_name.chars().count() > MAX_GROUP_NAME {
                return bad("nome de membro longo demais");
            }
            m.device_cert.verify()?;
            if m.device_cert.identity_pk != m.identity_pk
                || m.device_cert.node_id != *m.node_addr.id.as_bytes()
            {
                return bad("certificado de dispositivo não confere");
            }
        }
        if !self.has(&self.creator) {
            return bad("criador fora da lista");
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = postcard::to_stdvec(self)?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(Error::Protocol("retrato de grupo grande demais".into()));
        }
        Ok(bytes)
    }

    /// Decodifica e valida.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(Error::Protocol("retrato de grupo grande demais".into()));
        }
        let snap: Self = postcard::from_bytes(bytes)?;
        snap.validate()?;
        Ok(snap)
    }
}

/// Grupo como guardado no banco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGroup {
    pub snapshot: GroupSnapshot,
    /// Eu ainda sou membro no retrato mais novo que conheço.
    pub active: bool,
    pub updated_at: u64,
}

impl StoredGroup {
    pub fn id(&self) -> [u8; 32] {
        self.snapshot.id()
    }
}

/// Pares de rede vindos de grupos ativos: co-membros que **não** estão em `contacts`
/// (mutuamente). Sem repetição; vale o retrato de maior `seq`.
pub fn group_only_peers(
    groups: &[StoredGroup],
    me: &[u8; 32],
    contacts: &[Contact],
) -> Vec<Contact> {
    let mut best: Vec<(u64, Contact)> = Vec::new();
    for g in groups.iter().filter(|g| g.active) {
        for m in &g.snapshot.members {
            if &m.identity_pk == me
                || contacts
                    .iter()
                    .any(|c| c.is_mutual() && c.identity_pk == m.identity_pk)
            {
                continue;
            }
            match best
                .iter_mut()
                .find(|(_, c)| c.identity_pk == m.identity_pk)
            {
                Some((seq, c)) if *seq < g.snapshot.seq => {
                    *seq = g.snapshot.seq;
                    *c = m.as_peer();
                }
                Some(_) => {}
                None => best.push((g.snapshot.seq, m.as_peer())),
            }
        }
    }
    best.into_iter().map(|(_, c)| c).collect()
}

/// Grupos ativos em que `a` e `b` são membros (conversas a sincronizar entre os dois).
pub fn shared_groups<'a>(
    groups: &'a [StoredGroup],
    a: &[u8; 32],
    b: &[u8; 32],
) -> Vec<&'a StoredGroup> {
    groups
        .iter()
        .filter(|g| g.active && g.snapshot.has(a) && g.snapshot.has(b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    fn member(name: &str) -> (Identity, GroupMember) {
        let id = Identity::generate();
        let addr = EndpointAddr::new(id.endpoint_id());
        let m = GroupMember {
            identity_pk: id.identity_pk(),
            enc_pk: id.enc_pk(),
            device_cert: id.device_cert(1),
            node_addr: addr,
            display_name: name.into(),
        };
        (id, m)
    }

    fn contact_of(m: &GroupMember) -> Contact {
        m.as_peer()
    }

    #[test]
    fn id_is_bound_to_creator_and_nonce() {
        let (_, a) = member("a");
        let (_, b) = member("b");
        let n = [7u8; 16];
        assert_ne!(group_id(&a.identity_pk, &n), group_id(&b.identity_pk, &n));
        assert_ne!(
            group_id(&a.identity_pk, &n),
            group_id(&a.identity_pk, &[8u8; 16])
        );
    }

    #[test]
    fn a_valid_snapshot_roundtrips_and_can_evolve() {
        let (_, a) = member("ana");
        let (_, b) = member("beto");
        let snap =
            GroupSnapshot::create(a.identity_pk, "amigos", vec![a.clone(), b.clone()]).unwrap();
        assert_eq!(snap.seq, 1);
        assert_eq!(
            GroupSnapshot::decode(&snap.encode().unwrap()).unwrap(),
            snap
        );
        let (_, c) = member("cris");
        let next = snap
            .next(
                GroupOpKind::Add {
                    member: c.identity_pk,
                },
                snap.name.clone(),
                vec![a, b, c.clone()],
            )
            .unwrap();
        assert_eq!((next.seq, next.id()), (2, snap.id()));
        assert!(next.has(&c.identity_pk));
    }

    #[test]
    fn invalid_snapshots_are_rejected() {
        let (_, a) = member("ana");
        let (_, b) = member("beto");
        let ok = GroupSnapshot::create(a.identity_pk, "g", vec![a.clone(), b.clone()]).unwrap();

        // Criador fora da lista.
        let mut s = ok.clone();
        s.members.retain(|m| m.identity_pk != a.identity_pk);
        assert!(s.validate().is_err());
        // Membro repetido.
        let mut s = ok.clone();
        s.members.push(b.clone());
        assert!(s.validate().is_err());
        // Nome vazio / longo.
        let mut s = ok.clone();
        s.name = String::new();
        assert!(s.validate().is_err());
        s.name = "x".repeat(MAX_GROUP_NAME + 1);
        assert!(s.validate().is_err());
        // Mais de 20.
        let mut s = ok.clone();
        for i in 0..MAX_GROUP_MEMBERS {
            s.members.push(member(&format!("m{i}")).1);
        }
        assert!(s.validate().is_err());
        // Certificado de outro dispositivo.
        let mut s = ok.clone();
        s.members[1].node_addr = member("outro").1.node_addr;
        assert!(s.validate().is_err());
        // Lixo.
        assert!(GroupSnapshot::decode(&[1, 2, 3]).is_err());
    }

    #[test]
    fn group_only_peers_skip_me_and_contacts() {
        let (me, m_me) = member("eu");
        let (_, b) = member("beto");
        let (_, c) = member("cris");
        let snap = GroupSnapshot::create(
            m_me.identity_pk,
            "g",
            vec![m_me.clone(), b.clone(), c.clone()],
        )
        .unwrap();
        let groups = vec![StoredGroup {
            snapshot: snap,
            active: true,
            updated_at: 0,
        }];
        let only = group_only_peers(&groups, &me.identity_pk(), &[contact_of(&b)]);
        let ids: Vec<_> = only.iter().map(|p| p.identity_pk).collect();
        assert_eq!(ids, vec![c.identity_pk]);
        // Grupo em que saí/fui removido não gera pares.
        let mut inactive = groups.clone();
        inactive[0].active = false;
        assert!(group_only_peers(&inactive, &me.identity_pk(), &[]).is_empty());
        assert_eq!(
            shared_groups(&groups, &me.identity_pk(), &c.identity_pk).len(),
            1
        );
        assert!(shared_groups(&inactive, &me.identity_pk(), &c.identity_pk).is_empty());
    }
}
