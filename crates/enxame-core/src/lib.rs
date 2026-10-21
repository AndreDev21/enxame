//! Núcleo do Enxame. Clientes (CLI, nó headless, FFI) usam apenas [`Enxame`] e [`Event`].
//!
//! Módulos internos (`sync`, `carry`, `roles`, `blobs`, `store`, `crypto`) são preenchidos a
//! partir das próximas fases.

mod config;
mod contacts;
mod error;
mod event;
mod identity;
mod keystore;
mod net;
mod node;
mod proto;
mod role;

pub use config::{default_data_dir, Config, RelaySetting};
pub use contacts::{Contact, ContactStatus};
pub use error::{Error, Result};
pub use event::Event;
pub use identity::{fingerprint, unix_now, DeviceCert, Identity, InviteTicket};
#[cfg(feature = "os-keyring")]
pub use keystore::OsKeyStore;
pub use keystore::{open_keystore, FileKeyStore, KeyStore};
pub use net::{NatKind, NatReport, PathKind, PathMetrics};
pub use node::{
    list_contacts, AddOutcome, DiagReport, Enxame, PeerDiag, PendingRequest, DEFAULT_DIRECT_WAIT,
};
pub use proto::{ALPN_CONTACT, ALPN_SYNC};
pub use role::Role;

/// Versão do protocolo de wire. Negociada no `Hello`.
pub const PROTOCOL_VERSION: u8 = 1;
