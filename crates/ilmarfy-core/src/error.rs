use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// Sem conexão, DNS, timeout…
    #[error("sem conexão com o servidor: {0}")]
    Network(String),
    /// Token ausente, inválido ou revogado: o app deve pedir login de novo
    #[error("não autorizado: {0}")]
    Unauthorized(String),
    /// Muitas tentativas de senha
    #[error("{0}")]
    RateLimited(String),
    #[error("não encontrado: {0}")]
    NotFound(String),
    /// Outros erros HTTP, com a mensagem do servidor
    #[error("erro do servidor ({status}): {message}")]
    Http { status: u16, message: String },
    #[error("resposta inválida do servidor: {0}")]
    Decode(String),
    #[error("banco local: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("arquivo: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
