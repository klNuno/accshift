use super::*;
use crate::diagnostics::Diag;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn no_flags() -> SwitchOverrides {
    SwitchOverrides {
        online: false,
        invisible: false,
        graceful: false,
        force: false,
        admin: false,
        no_admin: false,
        launch_options: None,
    }
}

#[test]
fn switch_flags_left_unset_keep_the_gui_defaults() {
    assert_eq!(no_flags().into_steam(), SteamSwitchOverrides::default());
}

#[test]
fn each_switch_flag_maps_to_its_override() {
    let steam = SwitchOverrides {
        invisible: true,
        force: true,
        no_admin: true,
        launch_options: Some(" -x ".into()),
        ..no_flags()
    }
    .into_steam();
    assert_eq!(steam.persona, Some(PersonaMode::Invisible));
    assert_eq!(steam.shutdown_mode, Some(ShutdownMode::Force));
    assert_eq!(steam.run_as_admin, Some(false));
    assert_eq!(steam.launch_options.as_deref(), Some(" -x "));

    let steam = SwitchOverrides {
        online: true,
        graceful: true,
        admin: true,
        ..no_flags()
    }
    .into_steam();
    assert_eq!(steam.persona, Some(PersonaMode::Online));
    assert_eq!(steam.shutdown_mode, Some(ShutdownMode::Graceful));
    assert_eq!(steam.run_as_admin, Some(true));
}

/// Every subcommand the binary answers, one per `Command`/`Diag` variant.
/// Adding a subcommand means adding it here, and
/// `every_subcommand_is_listed` says so out loud when the count drifts.
fn every_command() -> Vec<Command> {
    vec![
        Command::List {
            platform: "steam".into(),
            folder: None,
        },
        Command::Platforms,
        Command::Switch {
            platform: "steam".into(),
            account_id: "alice".into(),
            online: false,
            invisible: false,
            graceful: false,
            force: false,
            admin: false,
            no_admin: false,
            launch_options: None,
        },
        Command::Descriptors,
        Command::DryRun {
            platform: "steam".into(),
            account_id: "alice".into(),
        },
        Command::Diag {
            action: Diag::Logs {
                codes: Vec::new(),
                level: None,
                op_id: None,
                run_id: None,
                platform: None,
                source: None,
                since: None,
                contains: None,
                limit: 1,
                all: false,
            },
        },
        Command::Diag {
            action: Diag::Explain {
                code: "no-such-code".into(),
            },
        },
        Command::Diag {
            action: Diag::Check,
        },
        Command::Diag {
            action: Diag::Level {
                module: None,
                set: None,
                reset: false,
                debug_for: None,
                stop_debug: false,
            },
        },
        Command::Diag {
            action: Diag::Bundle {
                records: 1,
                level: "info".into(),
                op_id: None,
                no_config: false,
                print: false,
            },
        },
        Command::Diag {
            action: Diag::Schema { write: None },
        },
    ]
}

/// Unique temp directory per test, removed on drop.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "accshift-cli-gate-test-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }

    fn entries(&self) -> usize {
        fs::read_dir(&self.0)
            .expect("read temp test dir")
            .filter_map(Result::ok)
            .count()
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_subcommand_is_listed() {
    let mut names: Vec<&str> = every_command().iter().map(Command::name).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "descriptors",
            "diag-bundle",
            "diag-check",
            "diag-explain",
            "diag-level",
            "diag-logs",
            "diag-schema",
            "dry-run",
            "list",
            "platforms",
            "switch",
        ],
        "a subcommand was added or renamed without updating every_command()"
    );
}

#[test]
fn the_exemption_list_is_empty() {
    assert!(
        CLI_GATE_EXEMPT.is_empty(),
        "an exemption was added: document it in docs/cli.md and say why the \
         command is safe for someone who deliberately switched the CLI off"
    );
}

#[test]
fn the_toggle_off_refuses_every_subcommand_the_same_way() {
    for command in every_command() {
        let name = command.name();
        assert_eq!(
            run(Format::Json, command, CliGate::Disabled),
            exit::CLI_DISABLED,
            "{name} ran with the CLI toggle off"
        );
    }
}

#[test]
fn unreadable_settings_refuse_every_subcommand_too() {
    for command in every_command() {
        let name = command.name();
        assert_eq!(
            run(
                Format::Json,
                command,
                CliGate::Unavailable("no home directory".into())
            ),
            exit::IO,
            "{name} ran without a settings file to check"
        );
    }
}

#[test]
fn a_refused_subcommand_writes_nothing() {
    // `diag schema --write <dir>` is the one subcommand whose writes land
    // somewhere a test can own, so it is the one that can prove a refusal
    // stops before the command body.
    let tmp = TempRoot::new("refused");

    let code = run(
        Format::Json,
        Command::Diag {
            action: Diag::Schema {
                write: Some(tmp.0.clone()),
            },
        },
        CliGate::Disabled,
    );

    assert_eq!(code, exit::CLI_DISABLED);
    assert_eq!(tmp.entries(), 0, "a refused run still wrote to disk");
}

#[test]
fn the_toggle_on_reaches_the_command() {
    let tmp = TempRoot::new("allowed");

    let code = run(
        Format::Json,
        Command::Diag {
            action: Diag::Schema {
                write: Some(tmp.0.clone()),
            },
        },
        CliGate::Allow,
    );

    assert_eq!(code, exit::OK);
    assert!(
        tmp.entries() > 0,
        "dispatch never reached the command with the toggle on"
    );
}
