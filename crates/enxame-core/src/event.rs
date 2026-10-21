use crate::Role;

/// Eventos emitidos pelo core. O stream não presume UI ativa (spec 17.1).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// O papel do nó mudou.
    RoleChanged { from: Role, to: Role },
    /// Alguém pediu para ser seu contato; responda com `Enxame::respond_contact`.
    ContactRequested {
        request_id: u64,
        display_name: String,
        fingerprint: String,
    },
    /// A aceitação mútua foi concluída.
    ContactAdded {
        display_name: String,
        fingerprint: String,
    },
}
