//! Where Steam is installed: detection, the user's override and folder checks.

#[allow(unused_imports)]
use super::*;

/// What a candidate folder looks like from Steam's point of view.
///
/// One classifier for the folder picker ([`set_steam_path`]) and for every
/// read path ([`resolve_steam_path`]), so a folder can never be accepted by
/// one and reported as "not installed" by the other. That split was the bug:
/// the picker took a folder holding only `steam.exe`, every later read
/// demanded `config/loginusers.vdf` and failed with a generic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamFolder {
    /// The path is missing, or is a file rather than a folder.
    NotADirectory,
    /// A real folder, but neither the Steam client nor a login history.
    NotSteam,
    /// Steam is installed here and has never signed in, so it has not written
    /// `config/loginusers.vdf` and there is no account to read yet.
    NeverSignedIn,
    /// `config/loginusers.vdf` is present: usable.
    Usable,
}

/// English text for the "installed but never signed in" case. The webview
/// shows a translated line keyed on [`STEAM_PATH_NEVER_SIGNED_IN`]; this is
/// what any other caller (and an untranslated fallback) gets.
pub(super) const NEVER_SIGNED_IN_MESSAGE: &str =
    "Steam found, sign in once so it creates its login history";

/// Machine-readable codes for the folder-picker rejections.
///
/// `PlatformError` serializes to the webview as its bare message string, so a
/// code travels inside that message, ahead of a `|` and an English fallback
/// (see [`coded_path_error`]). The frontend matches the code and translates
/// it; it never matches English prose.
pub const STEAM_PATH_NOT_A_DIRECTORY: &str = "steam_path_not_a_directory";
pub const STEAM_PATH_NOT_STEAM: &str = "steam_path_not_steam";
pub const STEAM_PATH_NEVER_SIGNED_IN: &str = "steam_path_never_signed_in";

pub fn classify_steam_folder(path: &Path) -> SteamFolder {
    if !path.is_dir() {
        return SteamFolder::NotADirectory;
    }
    if path.join("config").join("loginusers.vdf").is_file() {
        return SteamFolder::Usable;
    }
    if path.join(os::steam_executable_name()).is_file() {
        return SteamFolder::NeverSignedIn;
    }
    SteamFolder::NotSteam
}

/// `code|english fallback`, the format the webview parses.
pub(super) fn coded_path_error(code: &str, english: &str) -> PlatformError {
    PlatformError::new(
        PlatformErrorKind::ClientNotInstalled,
        format!("{code}|{english}"),
    )
}

pub(super) fn resolve_steam_path(app_handle: &dyn AppContext) -> Result<PathBuf, PlatformError> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.steam.path_override.trim();
    let steam_path = if !override_path.is_empty() {
        PathBuf::from(override_path)
    } else {
        // AppError::RegistryOpen maps to ClientNotInstalled.
        os::steam_installation_path()?
    };

    match classify_steam_folder(&steam_path) {
        SteamFolder::Usable => Ok(steam_path),
        // Plain prose, not a code: this one reaches a dozen commands whose
        // rejections the webview shows verbatim.
        SteamFolder::NeverSignedIn => Err(PlatformError::new(
            PlatformErrorKind::ClientNotInstalled,
            NEVER_SIGNED_IN_MESSAGE,
        )),
        SteamFolder::NotADirectory | SteamFolder::NotSteam => Err(PlatformError::new(
            PlatformErrorKind::ClientNotInstalled,
            "Could not locate Steam installation",
        )),
    }
}

pub fn get_steam_path(app_handle: AppCtx) -> Result<String, PlatformError> {
    let cfg = config::load_config(&app_handle);
    if !cfg.steam.path_override.trim().is_empty() {
        return Ok(cfg.steam.path_override);
    }
    resolve_steam_path(&app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_steam_path(app_handle: AppCtx, path: String) -> Result<(), PlatformError> {
    let trimmed = path.trim().to_string();
    // The override is later joined with steam.exe and launched, and every read
    // path resolves it through `classify_steam_folder`. Accept exactly what
    // those reads accept, so nothing can be saved here and then reported as
    // "not installed" a second later.
    if !trimmed.is_empty() {
        match classify_steam_folder(Path::new(&trimmed)) {
            SteamFolder::Usable => {}
            SteamFolder::NeverSignedIn => {
                return Err(coded_path_error(
                    STEAM_PATH_NEVER_SIGNED_IN,
                    NEVER_SIGNED_IN_MESSAGE,
                ));
            }
            SteamFolder::NotSteam => {
                return Err(coded_path_error(
                    STEAM_PATH_NOT_STEAM,
                    "This folder does not look like a Steam installation",
                ));
            }
            SteamFolder::NotADirectory => {
                return Err(coded_path_error(
                    STEAM_PATH_NOT_A_DIRECTORY,
                    "Steam path override must be an existing directory",
                ));
            }
        }
    }
    config::update_config(&app_handle, |cfg| {
        if trimmed.is_empty() {
            cfg.steam.path_override = String::new();
        } else {
            cfg.steam.path_override = trimmed;
        }
    })
    .map_err(Into::into)
}

pub fn select_steam_path() -> Result<String, PlatformError> {
    os::select_folder("Select Steam folder").map_err(Into::into)
}
