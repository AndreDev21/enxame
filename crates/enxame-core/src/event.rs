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
    /// Chegou uma mensagem nova de um contato; leia com `Enxame::history`.
    MessageReceived {
        conversation_id: [u8; 32],
        id: [u8; 32],
        from_name: String,
        from_fingerprint: String,
    },
    /// Progresso de um envio (`upload`) ou download de arquivo. `id` é a raiz do blob (zeros
    /// enquanto um envio ainda está sendo importado).
    TransferProgress {
        id: [u8; 32],
        name: String,
        done: u64,
        total: u64,
        upload: bool,
    },
    /// A aceitação mútua foi concluída.
    ContactAdded {
        display_name: String,
        fingerprint: String,
    },
}
