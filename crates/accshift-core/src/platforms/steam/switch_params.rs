//! The params a Steam switch takes: built here for the CLI (accshift-app's
//! `SteamLaunchConfig` sends the same keys for the GUI) and read back by
//! `SteamService` through [`SteamSwitchParams`].
//!
//! The rules: launch options trimmed and capped, and any shutdown mode but
//! `force` graceful.

use serde::Deserialize;
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

/// The `params` value of a Steam switch or setup, as `SteamService` reads it.
/// A missing key takes its default; a key of the wrong type is an error
/// rather than a silent default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SteamSwitchParams {
    pub run_as_admin: bool,
    pub launch_options: String,
    pub shutdown_mode: Option<String>,
    /// `online` or `invisible`; absent leaves the persona state alone.
    pub mode: Option<String>,
    pub steam_id: Option<String>,
}

impl SteamSwitchParams {
    pub fn from_value(params: Value) -> Result<Self, String> {
        // Callers with nothing to say send null.
        if params.is_null() {
            return Ok(Self::default());
        }
        serde_json::from_value(params).map_err(|e| format!("Invalid Steam switch params: {e}"))
    }

    pub fn force_kill(&self) -> bool {
        ShutdownMode::from_setting(self.shutdown_mode.as_deref()) == ShutdownMode::Force
    }
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
    fn the_service_reads_back_what_the_builder_writes() {
        let defaults = SteamSwitchDefaults::from_settings(true, "-silent", Some("force"));
        let params = steam_switch_params(
            &defaults,
            SteamSwitchOverrides {
                persona: Some(PersonaMode::Invisible),
                ..SteamSwitchOverrides::default()
            },
        );
        let parsed = SteamSwitchParams::from_value(params).unwrap();
        assert_eq!(
            parsed,
            SteamSwitchParams {
                run_as_admin: true,
                launch_options: "-silent".into(),
                shutdown_mode: Some("force".into()),
                mode: Some("invisible".into()),
                steam_id: None,
            }
        );
        assert!(parsed.force_kill());
    }

    #[test]
    fn missing_params_take_their_defaults_and_wrong_types_fail() {
        let parsed = SteamSwitchParams::from_value(json!({})).unwrap();
        assert_eq!(parsed, SteamSwitchParams::default());
        assert!(!parsed.force_kill());
        assert_eq!(
            SteamSwitchParams::from_value(Value::Null).unwrap(),
            SteamSwitchParams::default()
        );
        // The old by-hand parse read a string "true" as false without a word.
        assert!(SteamSwitchParams::from_value(json!({"runAsAdmin": "true"})).is_err());
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
