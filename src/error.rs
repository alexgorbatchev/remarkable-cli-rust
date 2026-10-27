use remarkable_cloud::{Error as Cloud, ErrorKind, UpdateState};
use std::{error::Error as _, io};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Cloud(#[from] Cloud),
    #[error(transparent)]
    Render(#[from] remarkable_render::Error),
    #[error(transparent)]
    Stroke(#[from] rmscene::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error("credentials file not found at {0}: run 'remarkable auth pair <code>' first")]
    Credentials(String),
    #[error("page index {index} out of bounds (document has {pages} pages)")]
    Page { index: usize, pages: usize },
    #[error("command deadline exceeded")]
    Deadline,
    #[error("command cancelled")]
    Cancelled,
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Commit(Box<Error>),
    #[error("{0}")]
    Interrupted(#[source] Box<Error>),
    #[error("{0}")]
    Context(String, #[source] Box<Error>),
    #[error("{}", joined(.0))]
    Multiple(Vec<Error>),
}
fn joined(errors: &[Error]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn message(value: impl Into<String>) -> Error {
    Error::Message(value.into())
}
pub fn after_commit(state: UpdateState, error: Error) -> Error {
    if matches!(state, UpdateState::CommitUnknown | UpdateState::Committed) {
        Error::Commit(Box::new(error))
    } else {
        error
    }
}
impl Error {
    pub fn context(self, context: impl Into<String>) -> Self {
        Self::Context(format!("{}: {self}", context.into()), Box::new(self))
    }
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::Commit(_) => 6,
            Self::Context(_, error) | Self::Interrupted(error) => error.exit_status(),
            Self::Multiple(errors) => errors
                .iter()
                .map(Self::exit_status)
                .max_by_key(|status| precedence(*status))
                .unwrap_or(1),
            Self::Credentials(_) => 4,
            Self::Cloud(error) => cloud_status(error),
            Self::Render(remarkable_render::Error::PageOutOfBounds { .. }) | Self::Page { .. } => 3,
            Self::Io(error) if error.kind() == io::ErrorKind::NotFound => 3,
            Self::Render(remarkable_render::Error::Io(error))
                if error.kind() == io::ErrorKind::NotFound =>
            {
                3
            }
            Self::Deadline => 5,
            _ => 1,
        }
    }
}
fn precedence(status: u8) -> u8 {
    match status {
        6 => 5,
        4 => 4,
        5 => 3,
        3 => 2,
        _ => 1,
    }
}
fn cloud_status(error: &Cloud) -> u8 {
    if let Cloud::Multiple(errors) = error {
        return errors
            .iter()
            .map(cloud_status)
            .max_by_key(|status| precedence(*status))
            .unwrap_or(1);
    }
    if error.kind() == Some(ErrorKind::Unauthorized) {
        return 4;
    }
    match error {
        Cloud::Status(error)
            if error.body_error.is_some()
                || error.status_code >= 500
                || [408, 429].contains(&error.status_code) =>
        {
            5
        }
        Cloud::AttemptTimeout(_) => 5,
        Cloud::Http(error) if failed_network(error) => 5,
        Cloud::Io(error) if error.kind() == io::ErrorKind::NotFound => 3,
        _ if error.kind() == Some(ErrorKind::ItemNotFound) => 3,
        _ => 1,
    }
}
fn failed_network(error: &reqwest::Error) -> bool {
    if error.is_timeout() || error.is_body() || error.is_decode() || error.is_dns() {
        return true;
    }
    let mut source = error.source();
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<io::Error>()
            && matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::NotConnected
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::HostUnreachable
                    | io::ErrorKind::NetworkUnreachable
            )
        {
            return true;
        }
        if let Some(error) = error.downcast_ref::<hyper::Error>()
            && (error.is_incomplete_message() || error.is_closed())
        {
            return true;
        }
        source = error.source();
    }
    false
}
