//! Núcleo do Enxame. Clientes (CLI, nó headless, FFI) usam apenas [`Enxame`] e [`Event`].
//!
//! Módulos internos (`sync`, `carry`, `roles`, `blobs`, `store`, `crypto`) são preenchidos a
//! partir das próximas fases.

mod config;
mod contacts;
mod crypto;
mod error;
mod event;
mod hlc;
mod identity;
mod keystore;
mod net;
mod node;
mod order;
mod proto;
mod role;
mod store;
mod sync;

pub use config::{default_data_dir, Config, RelaySetting};
pub use contacts::{Contact, ContactStatus};
pub use crypto::{dm_conversation_id, Envelope, EnvelopeKind, Recipient, SealedKey};
pub use error::{Error, Result};
pub use event::Event;
pub use hlc::{Hlc, HlcClock};
pub use identity::{fingerprint, unix_now, DeviceCert, Identity, InviteTicket};
#[cfg(feature = "os-keyring")]
pub use keystore::OsKeyStore;
pub use keystore::{open_keystore, FileKeyStore, KeyStore};
pub use net::{NatKind, NatReport, PathKind, PathMetrics};
pub use node::{
    list_contacts, AddOutcome, DiagReport, Enxame, Message, PeerDiag, PendingRequest, SentMessage,
    DEFAULT_DIRECT_WAIT,
};
pub use proto::{ALPN_CONTACT, ALPN_SYNC};
pub use role::Role;
pub use sync::SyncStats;

/// Versão do protocolo de wire. Negociada no `Hello`.
pub const PROTOCOL_VERSION: u8 = 1;

/// Utilitários só para testes de integração.
#[doc(hidden)]
pub mod testing {
    use crate::{contacts, Enxame};

    /// Serializa os contatos atuais no formato legado (`contacts.bin`) da Fase 1.
    pub fn legacy_contacts_bytes(node: &Enxame) -> Vec<u8> {
        contacts::legacy_bytes(&node.contacts())
    }
}
