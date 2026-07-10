use serde::Serialize;
use thiserror::Error;

/// OS/Steam-layer error. Kept as a separate type (rather than folded into
/// [`PlatformError`]) because `os/` uses it for primitives that have nothing
/// to do with Steam or platforms (secret storage, folder pickers, URL
/// opening), and because its variants format their own user-facing messages
/// at dozens of call sites. It bridges into [`PlatformError`] via `From`,
/// which maps variants to the closest [`PlatformErrorKind`].
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Could not locate Steam installation: {0}")]
    RegistryOpen(String),

    #[error("Could not read Steam configuration: {0}")]
    RegistryRead(String),

    #[error("Could not write Steam configuration: {0}")]
    RegistryWrite(String),

    #[error("Could not read or write file: {0}")]
    FileRead(String),

    #[error("Could not start process: {0}")]
    ProcessStart(String),

    #[error("Invalid SteamID64")]
    InvalidSteamId,

    #[error("User data folder not found")]
    UserdataNotFound(String),

    #[error("Could not resolve path: {0}")]
    PathResolve(String),

    #[error("Could not open folder: {0}")]
    FolderOpen(String),

    #[error("Steam is still running")]
    KillSteamTimeout,

    #[error("Steam is running as administrator")]
    SteamElevated,

    #[error("Dialog cancelled")]
    Cancelled,

    #[error("{0}")]
    UnsupportedOperatingSystem(String),
}

impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}

/// Machine-readable family for a [`PlatformError`].
///
/// Serialized as `snake_case` so it can be exposed to the frontend later
/// without renaming. Only a handful of creation sites are typed today
/// (Steam, the cross-process lock); everything migrated mechanically from
/// `Result<_, String>` lands on [`PlatformErrorKind::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformErrorKind {
    /// The platform's client/launcher is not installed or cannot be located.
    ClientNotInstalled,
    /// The client is (still) running and blocks the operation; retrying after
    /// it exits (or after closing the elevated instance) can succeed.
    ClientRunning,
    /// The requested account/profile is unknown to the platform.
    AccountNotFound,
    /// The setup session expired or was never registered.
    SetupExpired,
    /// The cross-process operation lock is held by another accshift instance.
    LockContended,
    /// Underlying filesystem/registry I/O failure.
    Io,
    /// Cryptography failure (secret encryption/decryption, snapshot crypto).
    Crypto,
    /// Unclassified — the default for errors migrated from plain strings.
    Other,
}

/// Structured error for platform operations (trait `PlatformService`, Tauri
/// platform commands, CLI).
///
/// Contract with the frontend: every `catch` around `invoke()` does
/// `String(e)` (or feeds `e` to a toast) and expects a plain string, so this
/// type serializes as its `Display` string — the exact message that used to
/// be the `Err(String)` payload. `kind` is not sent over IPC yet; it exists
/// for the CLI (exit codes) and for a later structured serialization.
#[derive(Debug, Clone)]
pub struct PlatformError {
    pub kind: PlatformErrorKind,
    pub message: String,
}

impl PlatformError {
    pub fn new(kind: PlatformErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Unclassified error ([`PlatformErrorKind::Other`]).
    pub fn other(message: impl Into<String>) -> Self {
        Self::new(PlatformErrorKind::Other, message)
    }
}

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PlatformError {}

/// Serialize as the bare message string: Tauri command rejections must reach
/// the webview as the same plain string they were before the typed-error
/// migration (the frontend renders rejections with `String(e)`).
impl Serialize for PlatformError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.message)
    }
}

impl From<String> for PlatformError {
    fn from(message: String) -> Self {
        Self::other(message)
    }
}

impl From<&str> for PlatformError {
    fn from(message: &str) -> Self {
        Self::other(message)
    }
}

/// Escape hatch for internal helpers that still return `Result<_, String>`:
/// lets a typed error propagate through them with `?` (the kind is dropped,
/// the message is preserved).
impl From<PlatformError> for String {
    fn from(e: PlatformError) -> String {
        e.message
    }
}

impl From<AppError> for PlatformError {
    fn from(e: AppError) -> Self {
        let kind = match &e {
            // RegistryOpen is only produced by steam_installation_path
            // lookups ("Could not locate Steam installation: ...").
            AppError::RegistryOpen(_) => PlatformErrorKind::ClientNotInstalled,
            // Steam refuses to die (timeout) or runs elevated: both mean the
            // client is still running and the operation can be retried once
            // the user deals with it.
            AppError::KillSteamTimeout | AppError::SteamElevated => {
                PlatformErrorKind::ClientRunning
            }
            AppError::RegistryRead(_)
            | AppError::RegistryWrite(_)
            | AppError::FileRead(_)
            | AppError::PathResolve(_)
            | AppError::FolderOpen(_)
            | AppError::ProcessStart(_) => PlatformErrorKind::Io,
            AppError::InvalidSteamId
            | AppError::UserdataNotFound(_)
            | AppError::Cancelled
            | AppError::UnsupportedOperatingSystem(_) => PlatformErrorKind::Other,
        };
        Self::new(kind, e.to_string())
    }
}

impl From<crate::lock::LockError> for PlatformError {
    fn from(e: crate::lock::LockError) -> Self {
        let kind = match &e {
            crate::lock::LockError::Contended => PlatformErrorKind::LockContended,
            crate::lock::LockError::Io(_) => PlatformErrorKind::Io,
        };
        Self::new(kind, e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression test: every variant that wraps an OS/detail string must surface it
    // through Display (and therefore through `to_string()` and `From<AppError> for
    // String`, which is what every Tauri command and the diagnostic logger relies on).
    // Losing the detail here means callers only ever see a generic sentence with no
    // way to tell a permission error from a locked file from a missing path.
    #[test]
    fn detail_carrying_variants_include_the_wrapped_string() {
        let detail = "access is denied (os error 5)";

        let cases: Vec<AppError> = vec![
            AppError::RegistryOpen(detail.to_string()),
            AppError::RegistryRead(detail.to_string()),
            AppError::RegistryWrite(detail.to_string()),
            AppError::FileRead(detail.to_string()),
            AppError::ProcessStart(detail.to_string()),
            AppError::PathResolve(detail.to_string()),
            AppError::FolderOpen(detail.to_string()),
        ];

        for case in cases {
            let message = case.to_string();
            assert!(
                message.contains(detail),
                "expected {message:?} to contain the wrapped detail {detail:?}"
            );
        }
    }

    #[test]
    fn file_read_message_is_not_login_specific() {
        // FileRead is reused for non-login file operations (bulk edit reads/writes,
        // game-settings copy staging), so its fixed text must not claim it is about
        // Steam login data.
        let err = AppError::FileRead("permission denied".to_string());
        let message = err.to_string();
        assert!(!message.contains("login"));
        assert!(message.contains("permission denied"));
    }

    #[test]
    fn variants_without_a_detail_string_keep_a_fixed_message() {
        assert_eq!(AppError::InvalidSteamId.to_string(), "Invalid SteamID64");
        assert_eq!(
            AppError::KillSteamTimeout.to_string(),
            "Steam is still running"
        );
        assert_eq!(
            AppError::SteamElevated.to_string(),
            "Steam is running as administrator"
        );
        assert_eq!(AppError::Cancelled.to_string(), "Dialog cancelled");
    }

    #[test]
    fn app_error_converts_into_string_via_display() {
        let err = AppError::RegistryRead("value not found".to_string());
        let as_string: String = err.into();
        assert_eq!(
            as_string,
            "Could not read Steam configuration: value not found"
        );
    }

    // -----------------------------------------------------------------------
    // PlatformError
    // -----------------------------------------------------------------------

    #[test]
    fn platform_error_display_is_the_bare_message() {
        let err = PlatformError::other("Could not locate Discord executable");
        assert_eq!(err.to_string(), "Could not locate Discord executable");
    }

    // Regression: the frontend renders command rejections with `String(e)` and
    // shows them in toasts verbatim. A PlatformError must therefore serialize
    // as a plain JSON string (the message), NOT as `{ kind, message }` —
    // otherwise every error toast would read "[object Object]".
    #[test]
    fn platform_error_serializes_as_plain_string_for_the_webview() {
        let err = PlatformError::new(PlatformErrorKind::ClientRunning, "Steam is still running");
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"Steam is still running\"");
    }

    #[test]
    fn platform_error_from_string_is_other_with_message_preserved() {
        let err = PlatformError::from("Battle.net account not found".to_string());
        assert_eq!(err.kind, PlatformErrorKind::Other);
        assert_eq!(err.message, "Battle.net account not found");
    }

    #[test]
    fn platform_error_roundtrips_through_string_keeping_message() {
        let err = PlatformError::new(PlatformErrorKind::SetupExpired, "Steam setup not found");
        let as_string: String = err.into();
        assert_eq!(as_string, "Steam setup not found");
    }

    #[test]
    fn app_error_maps_to_meaningful_platform_kinds() {
        let cases: Vec<(AppError, PlatformErrorKind)> = vec![
            (
                AppError::RegistryOpen("key missing".into()),
                PlatformErrorKind::ClientNotInstalled,
            ),
            (AppError::KillSteamTimeout, PlatformErrorKind::ClientRunning),
            (AppError::SteamElevated, PlatformErrorKind::ClientRunning),
            (AppError::FileRead("denied".into()), PlatformErrorKind::Io),
            (AppError::InvalidSteamId, PlatformErrorKind::Other),
        ];
        for (app_error, expected_kind) in cases {
            let message = app_error.to_string();
            let err = PlatformError::from(app_error);
            assert_eq!(err.kind, expected_kind);
            // Message must stay the exact Display output of the AppError.
            assert_eq!(err.message, message);
        }
    }

    #[test]
    fn lock_error_maps_contention_to_lock_contended() {
        let err = PlatformError::from(crate::lock::LockError::Contended);
        assert_eq!(err.kind, PlatformErrorKind::LockContended);
        assert_eq!(err.message, "Another accshift instance is holding the lock");

        let err = PlatformError::from(crate::lock::LockError::Io("bad fd".into()));
        assert_eq!(err.kind, PlatformErrorKind::Io);
    }

    #[test]
    fn platform_error_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&PlatformErrorKind::ClientNotInstalled).unwrap(),
            "\"client_not_installed\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformErrorKind::LockContended).unwrap(),
            "\"lock_contended\""
        );
    }
}
