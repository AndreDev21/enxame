/// Erros do core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Funcionalidade ainda não implementada na fase atual.
    #[error("não implementado: {0}")]
    Unimplemented(&'static str),
    /// Falha de E/S local (arquivos de configuração, chaves, contatos).
    #[error("erro de E/S: {0}")]
    Io(#[from] std::io::Error),
    /// Falha ao (de)serializar dados persistidos ou de wire.
    #[error("erro de codificação: {0}")]
    Codec(String),
    /// Configuração inválida.
    #[error("configuração inválida: {0}")]
    Config(String),
    /// Falha no keystore (do SO ou em arquivo).
    #[error("keystore: {0}")]
    KeyStore(String),
    /// Ticket malformado, de outra versão, expirado ou com assinatura inválida.
    #[error("ticket inválido: {0}")]
    InvalidTicket(String),
    /// Mensagem de protocolo inválida ou violação de regra de protocolo.
    #[error("violação de protocolo: {0}")]
    Protocol(String),
    /// Falha de rede (bind, conexão, stream).
    #[error("rede: {0}")]
    Net(String),
    /// Contato desconhecido ou ambíguo.
    #[error("contato: {0}")]
    Contact(String),
    /// Operação excedeu o tempo limite.
    #[error("tempo esgotado: {0}")]
    Timeout(&'static str),
}

impl From<postcard::Error> for Error {
    fn from(e: postcard::Error) -> Self {
        Self::Codec(e.to_string())
    }
}

/// Alias de resultado do core.
pub type Result<T> = std::result::Result<T, Error>;
