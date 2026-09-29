use serde::{Deserialize, Serialize};
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Deserialize, Serialize)]
pub struct Error {
    pub code: String,
    #[serde(default = "protocol")]
    pub rhyven_protocol: u32,
    #[serde(default)]
    pub kind: String,
    pub message: String,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            rhyven_protocol: 1,
            kind: kind(code).into(),
            message: message.into(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new("invalid_json", e.to_string())
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("database", e.to_string())
    }
}
pub fn ensure(condition: bool, code: &str, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::new(code, message))
    }
}

fn protocol() -> u32 {
    1
}
fn kind(code: &str) -> &'static str {
    match code {
        "not_found" | "not_installed" | "unknown_tool" | "unknown_function"
        | "unknown_operation" => "NOT_FOUND",
        "permission"
        | "permission_review_required"
        | "authentication"
        | "auth_required"
        | "approval"
        | "approval_required"
        | "approval_expired"
        | "approval_stale" => "PERMISSION_DENIED",
        "container_timeout" | "service_timeout" | "script_timeout" => "TIMEOUT",
        "container_unavailable"
        | "script_unavailable"
        | "service_unavailable"
        | "network"
        | "io"
        | "database" => "UNAVAILABLE",
        "app_error"
        | "script_failed"
        | "script_protocol"
        | "script_incomplete"
        | "container_failed"
        | "container_protocol"
        | "container_incomplete"
        | "service_protocol"
        | "service_incomplete"
        | "service_unclean_stop" => "APP_ERROR",
        _ => "INVALID_ARGUMENT",
    }
}
