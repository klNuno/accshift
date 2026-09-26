//! The params a Steam switch takes, built in one place for every caller.
//!
//! The GUI builds them from its settings store and the CLI from the same store
//! plus its flags. Both used to do it by hand and had drifted: the CLI passed
//! launch options untrimmed, and the GUI fell back to a force kill where the
//! settings schema and the CLI fall back to a graceful one. The rules here
//! follow the settings schema (`src/lib/platforms/steam/settingsSchema.ts`).

use serde_json::{json, Value};

/// The most launch options the settings schema keeps, in characters.
pub const MAX_LAUNCH_OPTIONS_CHARS: usize = 256;

/// How Steam is closed before the switch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ShutdownMode {
    #[default]
    Graceful,
    Force,
}

impl ShutdownMode {
    /// Anything but `force` is graceful, as in the settings schema.
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("force") => Self::Force,
            _ => Self::Graceful,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graceful => "graceful",
            Self::Force => "force",
        }
    }
}

/// The persona state Steam signs in with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonaMode {
    Online,
    Invisible,
}

impl PersonaMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Invisible => "invisible",
        }
    }
}

/// The Steam defaults the user saved in the GUI settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SteamSwitchDefaults {
    pub run_as_admin: bool,
    pub launch_options: String,
    pub shutdown_mode: ShutdownMode,
}

impl SteamSwitchDefaults {
    /// Normalises the raw settings fields the way the settings schema does:
    /// launch options trimmed and capped, an unknown shutdown mode graceful.
    pub fn from_settings(
        run_as_admin: bool,
        launch_options: &str,
        shutdown_mode: Option<&str>,
    ) -> Self {
        Self {
            run_as_admin,
            launch_options: launch_options
                .trim()
                .chars()
                .take(MAX_LAUNCH_OPTIONS_CHARS)
                .collect(),
            shutdown_mode: ShutdownMode::from_setting(shutdown_mode),
        }
    }
}

/// What one switch asks for on top of the defaults. `None` keeps the default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SteamSwitchOverrides {
    pub run_as_admin: Option<bool>,
    pub shutdown_mode: Option<ShutdownMode>,
    /// Only set when the caller asked for one: a plain switch must not touch
    /// the account's existing online or invisible state.
    pub persona: Option<PersonaMode>,
    /// An empty string is an override too: it launches with no options.
    pub launch_options: Option<String>,
}

/// The `params` value `SteamService::switch_account` reads.
pub fn steam_switch_params(
    defaults: &SteamSwitchDefaults,
    overrides: SteamSwitchOverrides,
) -> Value {
    let launch_options = overrides
        .launch_options
        .map(|options| options.trim().to_string())
        .unwrap_or_else(|| defaults.launch_options.clone());
    let mut params = json!({
        "runAsAdmin": overrides.run_as_admin.unwrap_or(defaults.run_as_admin),
        "launchOptions": launch_options,
        "shutdownMode": overrides.shutdown_mode.unwrap_or(defaults.shutdown_mode).as_str(),
    });
    if let Some(persona) = overrides.persona {
        params["mode"] = json!(persona.as_str());
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_switch_sends_the_saved_defaults_and_no_persona() {
        let defaults = SteamSwitchDefaults::from_settings(true, "  -silent  ", Some("force"));
        let params = steam_switch_params(&defaults, SteamSwitchOverrides::default());
        assert_eq!(
            params,
            json!({"runAsAdmin": true, "launchOptions": "-silent", "shutdownMode": "force"})
        );
    }

    #[test]
    fn missing_or_unknown_shutdown_modes_are_graceful() {
        for raw in [None, Some(""), Some("kill"), Some("graceful")] {
            let defaults = SteamSwitchDefaults::from_settings(false, "", raw);
            assert_eq!(defaults.shutdown_mode, ShutdownMode::Graceful, "{raw:?}");
        }
    }

    #[test]
    fn saved_launch_options_are_capped_like_the_settings_schema() {
        let long = "x".repeat(MAX_LAUNCH_OPTIONS_CHARS + 40);
        let defaults = SteamSwitchDefaults::from_settings(false, &long, None);
        assert_eq!(
            defaults.launch_options.chars().count(),
            MAX_LAUNCH_OPTIONS_CHARS
        );
    }

    #[test]
    fn overrides_win_and_an_empty_launch_option_clears_the_default() {
        let defaults = SteamSwitchDefaults::from_settings(true, "-silent", Some("force"));
        let params = steam_switch_params(
            &defaults,
            SteamSwitchOverrides {
                run_as_admin: Some(false),
                shutdown_mode: Some(ShutdownMode::Graceful),
                persona: Some(PersonaMode::Invisible),
                launch_options: Some(String::new()),
            },
        );
        assert_eq!(
            params,
            json!({
                "runAsAdmin": false,
                "launchOptions": "",
                "shutdownMode": "graceful",
                "mode": "invisible",
            })
        );
    }
}
