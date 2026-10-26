//! Núcleo do Enxame. Clientes (CLI, nó headless, FFI) usam apenas [`Enxame`] e [`Event`].
//!
//! Módulos internos (`sync`, `carry`, `roles`, `blobs`, `store`, `crypto`) são preenchidos a
//! partir das próximas fases.

mod blobs;
mod calls;
mod carry;
mod config;
mod contacts;
mod crypto;
mod error;
mod event;
mod filecrypt;
mod files;
mod groups;
mod hlc;
mod identity;
mod keystore;
mod media;
mod net;
mod node;
mod order;
mod proto;
mod role;
mod roles;
mod store;
mod sync;

pub use carry::CarryStats;
pub use config::{default_data_dir, Budget, Config, RelaySetting, RoleMode};
pub use contacts::{Contact, ContactStatus};
pub use crypto::{dm_conversation_id, Envelope, EnvelopeKind, Recipient, SealedKey};
pub use error::{Error, Result};
pub use event::Event;
pub use files::{guess_mime, human_size, sanitize_name, FileRef};
pub use groups::{GROUP_VERSION, MAX_GROUP_MEMBERS, MAX_GROUP_NAME};
pub use hlc::{Hlc, HlcClock};
pub use identity::{fingerprint, unix_now, DeviceCert, Identity, InviteTicket};
#[cfg(feature = "os-keyring")]
pub use keystore::OsKeyStore;
pub use keystore::{open_keystore, FileKeyStore, KeyStore};
pub use media::{BridgeCounters, BridgeRole, BridgeStats};
pub use net::{DnsChoice, NatKind, NatReport, PathKind, PathMetrics, FALLBACK_DNS};
pub use node::{
    list_contacts, read_history, AddOutcome, ContributionSummary, DiagReport, Downloaded, Enxame,
    FileEntry, GroupInfo, GroupMemberInfo, MediaBridge, Message, MessageFile, PeerDiag,
    PendingRequest, PrefetchedBlob, SentFile, SentGroupMessage, SentMessage, StartOptions,
    DEFAULT_DIRECT_WAIT,
};
pub use proto::{ALPN_CONTACT, ALPN_SYNC};
pub use role::Role;
#[cfg(target_os = "linux")]
pub use roles::LinuxSignals;
#[cfg(windows)]
pub use roles::WindowsSignals;
pub use roles::{
    effective_kbps_cap, target_role, CarryLimits, FallbackSignals, LinkKind, PlatformReading,
    PlatformSignals, RateLimiter, RoleEngine, RolePolicy, Signals, StaticSignals,
    ThroughputEstimator,
};
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
