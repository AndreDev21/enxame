//! Agenda de contatos: cache em memória com escrita direta no SQLite (`store`).
//! Até a Fase 1 ficava em `contacts.bin`; esse arquivo é importado uma vez (ver ADR 0005).

use std::path::Path;

use iroh::{EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};

use crate::{
    identity::{fingerprint, DeviceCert},
    store::{self, Store},
    Error, Result,
};

/// Formato legado (`contacts.bin`, Fase 1).
const LEGACY_FILE_VERSION: u8 = 1;

/// Estado da relação. Nenhum tráfego de carry/seed antes de `Mutual` (spec 6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactStatus {
    /// Importamos o ticket e enviamos o pedido; o outro lado ainda não aceitou.
    Outgoing,
    /// Aceitação mútua (`mutual = true`).
    Mutual,
}

/// Um contato: identidade e dispositivo conhecidos, referenciados separadamente (spec 6.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub identity_pk: [u8; 32],
    pub enc_pk: [u8; 32],
    pub device_cert: DeviceCert,
    pub node_addr: EndpointAddr,
    pub display_name: String,
    pub status: ContactStatus,
    pub added_at: u64,
}

impl Contact {
    /// `mutual = true` na terminologia da spec.
    pub fn is_mutual(&self) -> bool {
        self.status == ContactStatus::Mutual
    }

    /// Prefixo hexadecimal curto da identidade.
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.identity_pk)
    }

    /// `EndpointId` do dispositivo.
    pub fn endpoint_id(&self) -> EndpointId {
        self.node_addr.id
    }
}

#[derive(Serialize, Deserialize)]
struct LegacyFormat {
    version: u8,
    contacts: Vec<Contact>,
}

/// Serializa no formato legado (só para testes de migração).
pub fn legacy_bytes(contacts: &[Contact]) -> Vec<u8> {
    postcard::to_stdvec(&LegacyFormat {
        version: LEGACY_FILE_VERSION,
        contacts: contacts.to_vec(),
    })
    .unwrap_or_default()
}

/// Lê o `contacts.bin` da Fase 1, se existir.
pub fn read_legacy(path: &Path) -> Result<Option<Vec<Contact>>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let f: LegacyFormat = postcard::from_bytes(&bytes)
        .map_err(|e| Error::Codec(format!("{}: {e}", path.display())))?;
    if f.version != LEGACY_FILE_VERSION {
        return Err(Error::Codec(format!(
            "{}: versão {} não suportada",
            path.display(),
            f.version
        )));
    }
    Ok(Some(f.contacts))
}

/// Coleção de contatos com escrita direta no banco.
#[derive(Debug)]
pub struct ContactBook {
    store: Store,
    contacts: Vec<Contact>,
}

impl ContactBook {
    /// Carrega os contatos do banco.
    pub fn open(store: Store) -> Result<Self> {
        let contacts = store.with(|c| store::contacts_load(c))?;
        Ok(Self { store, contacts })
    }

    pub fn list(&self) -> &[Contact] {
        &self.contacts
    }

    pub fn by_identity(&self, pk: &[u8; 32]) -> Option<&Contact> {
        self.contacts.iter().find(|c| &c.identity_pk == pk)
    }

    /// Contato mútuo cujo dispositivo é `id`. É a porta de entrada de qualquer tráfego além do
    /// pedido de contato.
    pub fn mutual_by_endpoint(&self, id: &EndpointId) -> Option<&Contact> {
        self.contacts
            .iter()
            .find(|c| c.is_mutual() && &c.node_addr.id == id)
    }

    /// Resolve por prefixo da fingerprint (mín. 4 caracteres) ou nome exato.
    pub fn resolve(&self, query: &str) -> Result<&Contact> {
        let q = query.trim().to_lowercase();
        let hits: Vec<&Contact> = self
            .contacts
            .iter()
            .filter(|c| {
                c.display_name.to_lowercase() == q
                    || (q.len() >= 4 && c.fingerprint().starts_with(&q))
            })
            .collect();
        match hits.as_slice() {
            [one] => Ok(one),
            [] => Err(Error::Contact(format!("nenhum contato `{query}`"))),
            _ => Err(Error::Contact(format!("`{query}` é ambíguo"))),
        }
    }

    /// Insere ou atualiza (por identidade) e persiste. Um contato `Mutual` nunca volta a
    /// `Outgoing`.
    pub fn upsert(&mut self, mut contact: Contact) -> Result<()> {
        if let Some(existing) = self
            .contacts
            .iter()
            .find(|c| c.identity_pk == contact.identity_pk)
        {
            if existing.is_mutual() {
                contact.status = ContactStatus::Mutual;
            }
            contact.added_at = existing.added_at;
        }
        let c = contact.clone();
        self.store.with(|conn| store::contact_upsert(conn, &c))?;
        match self
            .contacts
            .iter_mut()
            .find(|x| x.identity_pk == contact.identity_pk)
        {
            Some(existing) => *existing = contact,
            None => self.contacts.push(contact),
        }
        Ok(())
    }

    /// Remove por identidade (pedido rejeitado) e persiste.
    pub fn remove(&mut self, pk: &[u8; 32]) -> Result<()> {
        self.store.with(|conn| store::contact_remove(conn, pk))?;
        self.contacts.retain(|c| &c.identity_pk != pk);
        Ok(())
    }
}
