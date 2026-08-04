use super::events::{
    code_from, platform_code, Event, TelemetryContext, CLI_COMMANDS, ERROR_CODES, OPERATIONS,
    UI_LANGUAGES,
};
use super::time::to_rfc3339_utc;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::time::{Duration, SystemTime};

/// Inert placeholder used when `ACCSHIFT_TELEMETRY_URL` is not set at compile
/// time. Not a live endpoint on purpose: it only marks builds that forgot to
/// set the variable so telemetry calls fail instead of silently pointing at
/// someone else's infrastructure.
const TELEMETRY_URL_FALLBACK: &str = "https://telemetry.invalid";

/// Telemetry Worker URL.
///
/// Must be set at compile time via `ACCSHIFT_TELEMETRY_URL=...`.
pub const TELEMETRY_URL: &str = match option_env!("ACCSHIFT_TELEMETRY_URL") {
    Some(s) => s,
    None => TELEMETRY_URL_FALLBACK,
};

/// User-Agent sent with every request.
/// Must match `UA_PREFIX` on the Worker (rejected otherwise).
pub fn user_agent(app_version: &str) -> String {
    format!("Accshift/{app_version} (telemetry)")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    A,
    B,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::A => "A",
            Mode::B => "B",
        }
    }
}

/// Maximum length of a version string sent as a property.
const MAX_VERSION_LEN: usize = 32;

/// Reduces a version string to digits, letters, dots and dashes.
///
/// The updater hands us whatever the release manifest declared, which is
/// remote input. It has never been anything but a semver string, and this is
/// what keeps it that way.
fn sanitize_version(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '+')
        .take(MAX_VERSION_LEN)
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Serializes an event to flat JSON for the `/track` endpoint.
///
/// `client_ts` is the instant the event happened, not the instant the batch
/// leaves: a batch covers up to a full flush interval, so a single
/// server-side stamp would flatten it.
pub fn event_to_json(event: &Event, ctx: &TelemetryContext, client_ts: SystemTime) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), Value::from(event.name()));
    m.insert("app_version".into(), Value::from(ctx.app_version.clone()));
    m.insert("os".into(), Value::from(ctx.os.clone()));
    m.insert("arch".into(), Value::from(ctx.arch.clone()));
    m.insert("os_version".into(), Value::from(ctx.os_version.clone()));
    m.insert("surface".into(), Value::from(ctx.surface.clone()));
    m.insert("client_ts".into(), Value::from(to_rfc3339_utc(client_ts)));
    if let Some(locale) = &ctx.locale {
        m.insert("locale".into(), Value::from(locale.clone()));
    }
    match event {
        Event::Ping { dropped_events } => {
            // Only sent when non-zero: a queue that never overflowed must not
            // add a property to every single ping just to say so.
            if *dropped_events > 0 {
                m.insert("dropped_events".into(), Value::from(*dropped_events));
            }
        }
        Event::FirstRun => {}
        Event::AppLaunched { duration_ms } => {
            m.insert("duration_ms".into(), Value::from(*duration_ms));
        }
        Event::PlatformSwitch {
            platform,
            duration_ms,
            success,
            error_code,
        } => {
            m.insert("platform".into(), Value::from(platform_code(platform)));
            m.insert("duration_ms".into(), Value::from(*duration_ms));
            m.insert("success".into(), Value::from(*success));
            // `count` carried the success flag before `success` existed, and
            // every dashboard built against it still reads it. Kept as-is.
            m.insert("count".into(), Value::from(u64::from(*success)));
            if let Some(code) = error_code {
                m.insert(
                    "error_code".into(),
                    Value::from(code_from(code, ERROR_CODES)),
                );
            }
        }
        Event::PersonaSwitch {
            platforms,
            succeeded,
        } => {
            m.insert("platforms".into(), Value::from(*platforms));
            m.insert("succeeded".into(), Value::from(*succeeded));
            // Same continuity reason as platform_switch above.
            m.insert("count".into(), Value::from(*succeeded));
        }
        Event::AccountAddStarted { platform }
        | Event::AccountAddCancelled { platform }
        | Event::AccountAdded { platform } => {
            m.insert("platform".into(), Value::from(platform_code(platform)));
        }
        Event::OperationFailed {
            operation,
            platform,
            error_code,
        } => {
            m.insert(
                "operation".into(),
                Value::from(code_from(operation, OPERATIONS)),
            );
            m.insert(
                "error_code".into(),
                Value::from(code_from(error_code, ERROR_CODES)),
            );
            if let Some(platform) = platform {
                m.insert("platform".into(), Value::from(platform_code(platform)));
            }
        }
        Event::Update {
            target_version,
            error_code,
            ..
        } => {
            if let Some(version) = target_version.as_deref().and_then(sanitize_version) {
                m.insert("target_version".into(), Value::from(version));
            }
            if let Some(code) = error_code {
                m.insert(
                    "error_code".into(),
                    Value::from(code_from(code, ERROR_CODES)),
                );
            }
        }
        Event::CliCommand {
            command,
            success,
            error_code,
        } => {
            m.insert(
                "command".into(),
                Value::from(code_from(command, CLI_COMMANDS)),
            );
            m.insert("success".into(), Value::from(*success));
            if let Some(code) = error_code {
                m.insert(
                    "error_code".into(),
                    Value::from(code_from(code, ERROR_CODES)),
                );
            }
        }
        Event::StreamerModeActivated => {}
        Event::DeepLinkUsed => {}
        Event::SessionEnded { duration_ms } => {
            m.insert("duration_ms".into(), Value::from(*duration_ms));
        }
        Event::AccountsSnapshot { platform, count } => {
            m.insert("platform".into(), Value::from(platform_code(platform)));
            m.insert("count".into(), Value::from(*count));
        }
        Event::SettingsSnapshot {
            ui_language,
            enabled_platforms,
            personas_enabled,
            pin_enabled,
            cli_enabled,
            deep_links_enabled,
            streamer_mode,
            animations,
        } => {
            m.insert(
                "ui_language".into(),
                Value::from(code_from(ui_language, UI_LANGUAGES)),
            );
            let platforms: Vec<Value> = enabled_platforms
                .iter()
                .map(|id| Value::from(platform_code(id)))
                .collect();
            m.insert("enabled_platforms".into(), Value::Array(platforms));
            m.insert("personas_enabled".into(), Value::from(*personas_enabled));
            m.insert("pin_enabled".into(), Value::from(*pin_enabled));
            m.insert("cli_enabled".into(), Value::from(*cli_enabled));
            m.insert(
                "deep_links_enabled".into(),
                Value::from(*deep_links_enabled),
            );
            m.insert(
                "streamer_mode".into(),
                Value::from(code_from(streamer_mode, &["auto", "off"])),
            );
            m.insert(
                "animations".into(),
                Value::from(code_from(animations, &["system", "on", "off"])),
            );
        }
    }
    Value::Object(m)
}

#[derive(Serialize)]
struct TrackPayload<'a> {
    mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anonymous_id: Option<&'a str>,
    events: Vec<Value>,
}

/// Synchronous POST to `/track`. Returns Ok on 2xx, Err otherwise.
///
/// Short timeouts (~5s) so the background thread is not blocked too long.
pub fn send_batch(
    client: &reqwest::blocking::Client,
    base_url: &str,
    user_agent: &str,
    mode: Mode,
    identifier: Option<&str>,
    events_json: Vec<Value>,
) -> Result<(), String> {
    if events_json.is_empty() {
        return Ok(());
    }
    let payload = TrackPayload {
        mode: mode.as_str(),
        install_id: (mode == Mode::B).then_some(identifier).flatten(),
        anonymous_id: (mode == Mode::A).then_some(identifier).flatten(),
        events: events_json,
    };
    let url = format!("{base_url}/track");
    let res = client
        .post(&url)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(5))
        .json(&payload)
        .send()
        .map_err(|e| format!("send: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("status: {}", res.status()));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentChoice {
    Refused,
    Basic,
    Enhanced,
}

impl ConsentChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::Refused => "refused",
            Self::Basic => "basic",
            Self::Enhanced => "enhanced",
        }
    }
}

#[derive(Serialize)]
struct ConsentPayload<'a> {
    choice: &'static str,
    app_version: &'a str,
}

/// Increments an aggregate onboarding-choice counter. This intentionally
/// carries no installation identifier, including when the choice is refusal.
pub fn record_consent_choice(
    client: &reqwest::blocking::Client,
    base_url: &str,
    user_agent: &str,
    choice: ConsentChoice,
    app_version: &str,
) -> Result<(), String> {
    let url = format!("{base_url}/consent");
    let payload = ConsentPayload {
        choice: choice.as_str(),
        app_version,
    };
    let res = client
        .post(&url)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(5))
        .json(&payload)
        .send()
        .map_err(|e| format!("send: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("status: {}", res.status()));
    }
    Ok(())
}

/// Calls `/forget` to delete data associated with an install_id.
pub fn forget(
    client: &reqwest::blocking::Client,
    base_url: &str,
    user_agent: &str,
    install_id: &str,
) -> Result<(), String> {
    let url = format!("{base_url}/forget");
    let res = client
        .post(&url)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(10))
        .json(&json!({ "install_id": install_id }))
        .send()
        .map_err(|e| format!("send: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("status: {}", res.status()));
    }
    Ok(())
}

/// Calls `/export` to retrieve raw JSON data for an install_id.
pub fn export(
    client: &reqwest::blocking::Client,
    base_url: &str,
    user_agent: &str,
    install_id: &str,
) -> Result<Value, String> {
    let url = format!("{base_url}/export");
    let res = client
        .post(&url)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(15))
        .json(&json!({ "install_id": install_id }))
        .send()
        .map_err(|e| format!("send: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("status: {}", res.status()));
    }
    res.json().map_err(|e| format!("parse: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::events::UpdateStage;
    use std::time::UNIX_EPOCH;

    fn ctx_with_locale(locale: Option<&str>) -> TelemetryContext {
        TelemetryContext {
            app_version: "0.9.0".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            os_version: "Windows 11 22631".into(),
            locale: locale.map(str::to_string),
            surface: "gui".into(),
        }
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn event_to_json_ping_has_only_invariants() {
        let ctx = ctx_with_locale(Some("fr-FR"));
        let v = event_to_json(&Event::Ping { dropped_events: 0 }, &ctx, at(1_785_846_896));
        assert_eq!(v["name"], "ping");
        assert_eq!(v["app_version"], "0.9.0");
        assert_eq!(v["os"], "windows");
        assert_eq!(v["arch"], "x86_64");
        assert_eq!(v["os_version"], "Windows 11 22631");
        assert_eq!(v["surface"], "gui");
        assert_eq!(v["locale"], "fr-FR");
        assert_eq!(v["client_ts"], "2026-08-04T12:34:56Z");
        assert!(v.get("duration_ms").is_none());
        assert!(v.get("platform").is_none());
        // A queue that never overflowed says nothing rather than zero.
        assert!(v.get("dropped_events").is_none());
    }

    #[test]
    fn event_to_json_ping_reports_dropped_events_when_any() {
        let ctx = ctx_with_locale(None);
        let v = event_to_json(&Event::Ping { dropped_events: 12 }, &ctx, at(0));
        assert_eq!(v["dropped_events"], 12);
    }

    #[test]
    fn event_to_json_platform_switch_keeps_count_and_adds_success() {
        let ctx = ctx_with_locale(None);
        let ev = Event::PlatformSwitch {
            platform: "steam".into(),
            duration_ms: 180,
            success: true,
            error_code: None,
        };
        let v = event_to_json(&ev, &ctx, at(0));
        assert_eq!(v["name"], "platform_switch");
        assert_eq!(v["platform"], "steam");
        assert_eq!(v["duration_ms"], 180);
        assert_eq!(v["success"], true);
        // Continuity: dashboards built before `success` existed read `count`.
        assert_eq!(v["count"], 1);
        assert!(v.get("error_code").is_none());
    }

    #[test]
    fn event_to_json_platform_switch_classifies_a_failure() {
        let ctx = ctx_with_locale(None);
        let ev = Event::PlatformSwitch {
            platform: "riot".into(),
            duration_ms: 40,
            success: false,
            error_code: Some("client_running".into()),
        };
        let v = event_to_json(&ev, &ctx, at(0));
        assert_eq!(v["success"], false);
        assert_eq!(v["count"], 0);
        assert_eq!(v["error_code"], "client_running");
    }

    #[test]
    fn event_to_json_never_lets_a_message_through_a_code_field() {
        let ctx = ctx_with_locale(None);
        let ev = Event::OperationFailed {
            operation: "profile_capture".into(),
            platform: Some("riot".into()),
            error_code: r"C:\Users\alice\riot missing".to_string(),
        };
        let v = event_to_json(&ev, &ctx, at(0));
        assert_eq!(v["operation"], "profile_capture");
        assert_eq!(v["platform"], "riot");
        // A raw message is not normalized into a code, it is discarded.
        assert_eq!(v["error_code"], "other");
    }

    #[test]
    fn event_to_json_rejects_an_unknown_platform_id() {
        let ctx = ctx_with_locale(None);
        let v = event_to_json(
            &Event::AccountAdded {
                platform: "MyPrivateLauncher".into(),
            },
            &ctx,
            at(0),
        );
        assert_eq!(v["platform"], "other");
    }

    #[test]
    fn event_to_json_keeps_the_battle_net_id_verbatim() {
        // Renaming it on the wire would break the dashboards a second time.
        let ctx = ctx_with_locale(None);
        let v = event_to_json(
            &Event::AccountAdded {
                platform: "battle-net".into(),
            },
            &ctx,
            at(0),
        );
        assert_eq!(v["platform"], "battle-net");
    }

    #[test]
    fn event_to_json_persona_switch_carries_counts_only() {
        let ctx = ctx_with_locale(None);
        let ev = Event::PersonaSwitch {
            platforms: 3,
            succeeded: 2,
        };
        let v = event_to_json(&ev, &ctx, at(0));
        assert_eq!(v["name"], "persona_switch");
        assert_eq!(v["platforms"], 3);
        assert_eq!(v["succeeded"], 2);
        assert_eq!(v["count"], 2);
        // Non-PII by construction: no persona name, no platform id, no account.
        assert!(v.get("platform").is_none());
    }

    #[test]
    fn event_to_json_feature_events_carry_no_pii() {
        let ctx = ctx_with_locale(None);
        let v = event_to_json(
            &Event::AccountAdded {
                platform: "discord".into(),
            },
            &ctx,
            at(0),
        );
        assert_eq!(v["name"], "account_added");
        // Platform id only: no account name, id, or display name.
        assert_eq!(v["platform"], "discord");
        assert!(v.get("account").is_none());

        let v = event_to_json(&Event::StreamerModeActivated, &ctx, at(0));
        assert_eq!(v["name"], "streamer_mode_activated");
        assert!(v.get("platform").is_none());

        let v = event_to_json(&Event::DeepLinkUsed, &ctx, at(0));
        assert_eq!(v["name"], "deep_link_used");
        // No URL contents: a deep link carries account ids in its path.
        assert!(v.get("url").is_none());
        assert!(v.get("platform").is_none());
    }

    #[test]
    fn event_to_json_update_sanitizes_the_remote_version() {
        let ctx = ctx_with_locale(None);
        let v = event_to_json(
            &Event::Update {
                stage: UpdateStage::Available,
                target_version: Some("1.4.2".into()),
                error_code: None,
            },
            &ctx,
            at(0),
        );
        assert_eq!(v["name"], "update_available");
        assert_eq!(v["target_version"], "1.4.2");

        // The manifest is remote input; it never reaches PostHog verbatim.
        let v = event_to_json(
            &Event::Update {
                stage: UpdateStage::Failed,
                target_version: Some("1.4.2 <script>alert(1)</script>".into()),
                error_code: Some("download failed".into()),
            },
            &ctx,
            at(0),
        );
        assert_eq!(v["name"], "update_failed");
        assert_eq!(v["target_version"], "1.4.2scriptalert1script");
        assert_eq!(v["error_code"], "download_failed");
    }

    #[test]
    fn event_to_json_settings_snapshot_is_all_low_cardinality_codes() {
        let ctx = ctx_with_locale(None);
        let v = event_to_json(
            &Event::SettingsSnapshot {
                ui_language: "pt-BR".into(),
                enabled_platforms: vec!["steam".into(), "battle-net".into()],
                personas_enabled: true,
                pin_enabled: false,
                cli_enabled: true,
                deep_links_enabled: true,
                streamer_mode: "auto".into(),
                animations: "system".into(),
            },
            &ctx,
            at(0),
        );
        assert_eq!(v["name"], "settings_snapshot");
        assert_eq!(v["ui_language"], "pt_br");
        assert_eq!(v["enabled_platforms"], json!(["steam", "battle-net"]));
        assert_eq!(v["personas_enabled"], true);
        assert_eq!(v["pin_enabled"], false);
        // A user-named custom theme has no field to travel in.
        assert!(v.get("theme").is_none());
    }

    #[test]
    fn event_to_json_cli_command_carries_no_arguments() {
        let ctx = TelemetryContext {
            surface: "cli".into(),
            ..ctx_with_locale(None)
        };
        let v = event_to_json(
            &Event::CliCommand {
                command: "switch".into(),
                success: false,
                error_code: Some("account_not_found".into()),
            },
            &ctx,
            at(0),
        );
        assert_eq!(v["name"], "cli_command");
        assert_eq!(v["surface"], "cli");
        assert_eq!(v["command"], "switch");
        assert_eq!(v["success"], false);
        // The CLI's own `unknown_account` exit code maps onto the one error
        // vocabulary shared with the app, so both surfaces stay comparable.
        assert_eq!(v["error_code"], "account_not_found");
        // The account id and every other argument stay on the machine.
        assert!(v.get("account_id").is_none());
        assert!(v.get("args").is_none());
    }

    #[test]
    fn event_to_json_omits_locale_when_none() {
        let ctx = ctx_with_locale(None);
        let v = event_to_json(&Event::Ping { dropped_events: 0 }, &ctx, at(0));
        assert!(v.get("locale").is_none());
    }

    #[test]
    fn user_agent_format() {
        assert_eq!(user_agent("0.9.0"), "Accshift/0.9.0 (telemetry)");
    }

    #[test]
    fn track_payload_uses_mode_specific_identifier_field() {
        let events = vec![json!({ "name": "ping" })];
        let mode_a = serde_json::to_value(TrackPayload {
            mode: Mode::A.as_str(),
            install_id: None,
            anonymous_id: Some("797f20fe-94de-4e89-98a2-ae3a3273ad1e"),
            events: events.clone(),
        })
        .unwrap();
        assert!(mode_a.get("install_id").is_none());
        assert_eq!(
            mode_a["anonymous_id"],
            "797f20fe-94de-4e89-98a2-ae3a3273ad1e"
        );

        let mode_b = serde_json::to_value(TrackPayload {
            mode: Mode::B.as_str(),
            install_id: Some("550e8400-e29b-41d4-a716-446655440000"),
            anonymous_id: None,
            events,
        })
        .unwrap();
        assert!(mode_b.get("anonymous_id").is_none());
        assert_eq!(mode_b["install_id"], "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn consent_choice_wire_values_are_stable() {
        assert_eq!(ConsentChoice::Refused.as_str(), "refused");
        assert_eq!(ConsentChoice::Basic.as_str(), "basic");
        assert_eq!(ConsentChoice::Enhanced.as_str(), "enhanced");
    }

    #[test]
    fn consent_payload_contains_no_identifier() {
        let payload = serde_json::to_value(ConsentPayload {
            choice: ConsentChoice::Refused.as_str(),
            app_version: "1.0.0",
        })
        .unwrap();
        assert_eq!(
            payload,
            json!({ "choice": "refused", "app_version": "1.0.0" })
        );
        assert!(payload.get("anonymous_id").is_none());
        assert!(payload.get("install_id").is_none());
    }

    #[test]
    fn telemetry_url_fallback_does_not_leak_private_infrastructure() {
        // Checked against the fallback constant directly (not TELEMETRY_URL,
        // which may resolve to a real build-time override) so this guard
        // holds regardless of whether ACCSHIFT_TELEMETRY_URL is set for this
        // test run. Guards against reintroducing a hardcoded private domain.
        assert!(!TELEMETRY_URL_FALLBACK.contains("mtsu"));
        assert!(TELEMETRY_URL_FALLBACK.ends_with(".invalid"));
    }
}
