//! Agenda de contatos. Persistência transitória em arquivo versionado (`contacts.bin`);
//! migra para o SQLite do `store` na Fase 2 (ver ADR 0004).

use std::path::PathBuf;

use iroh::{EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};

use crate::{
    identity::{fingerprint, DeviceCert},
    Error, Result,
};

const FILE_VERSION: u8 = 1;

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
struct FileFormat {
    version: u8,
    contacts: Vec<Contact>,
}

/// Coleção persistida de contatos.
#[derive(Debug)]
pub struct ContactBook {
    path: Option<PathBuf>,
    contacts: Vec<Contact>,
}

impl ContactBook {
    /// Abre (ou cria vazia) a agenda no caminho dado.
    pub fn open(path: PathBuf) -> Result<Self> {
        let contacts = match std::fs::read(&path) {
            Ok(bytes) => {
                let f: FileFormat = postcard::from_bytes(&bytes)
                    .map_err(|e| Error::Codec(format!("{}: {e}", path.display())))?;
                if f.version != FILE_VERSION {
                    return Err(Error::Codec(format!(
                        "{}: versão {} não suportada",
                        path.display(),
                        f.version
                    )));
                }
                f.contacts
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path: Some(path),
            contacts,
        })
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
        match self
            .contacts
            .iter_mut()
            .find(|c| c.identity_pk == contact.identity_pk)
        {
            Some(existing) => {
                if existing.is_mutual() {
                    contact.status = ContactStatus::Mutual;
                }
                contact.added_at = existing.added_at;
                *existing = contact;
            }
            None => self.contacts.push(contact),
        }
        self.save()
    }

    /// Remove por identidade (pedido rejeitado) e persiste.
    pub fn remove(&mut self, pk: &[u8; 32]) -> Result<()> {
        self.contacts.retain(|c| &c.identity_pk != pk);
        self.save()
    }

    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes = postcard::to_stdvec(&FileFormat {
            version: FILE_VERSION,
            contacts: self.contacts.clone(),
        })?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}
