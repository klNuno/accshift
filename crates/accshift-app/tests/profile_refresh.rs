//! `src/lib/app/useProfileRefresh.test.ts`, ported over a fake
//! [`RefreshEnv`]: each fake adapter answers from closures and records the
//! calls the TS mocks checked.

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::accounts::refresh::{refresh_avatars_now, refresh_bans_now, RefreshEnv};
use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::error::BackendError;
use accshift_app::platforms::warnings::WarningLoad;
use accshift_app::platforms::{
    PlatformAccount, PlatformCapabilities, PlatformDef, PlatformProfileInfo, ProfileRefresh,
    WarningLoadOptions, WarningRefreshOutcome,
};
use accshift_app::text::Text;
use std::collections::BTreeMap;

type ProfileFn = fn(&str) -> Result<Option<PlatformProfileInfo>, BackendError>;

#[derive(Default)]
struct FakeAdapter {
    accounts: Option<Result<Vec<PlatformAccount>, BackendError>>,
    profile: Option<ProfileFn>,
    warnings: Option<WarningLoad>,
    no_accounts: Option<Text>,
}

#[derive(Default)]
struct Env {
    adapters: BTreeMap<String, FakeAdapter>,
    ensured: Vec<String>,
    profile_calls: Vec<String>,
    warning_calls: Vec<(Vec<String>, WarningLoadOptions)>,
}

impl RefreshEnv for Env {
    type Adapter = String;

    fn ensure(&mut self, platform_id: &str) -> Option<String> {
        self.ensured.push(platform_id.into());
        self.adapters
            .contains_key(platform_id)
            .then(|| platform_id.to_string())
    }

    fn has_profile_info(&self, adapter: &String) -> bool {
        self.adapters[adapter].profile.is_some()
    }

    fn has_warnings(&self, adapter: &String) -> bool {
        self.adapters[adapter].warnings.is_some()
    }

    fn load_accounts(&mut self, adapter: &String) -> Result<Vec<PlatformAccount>, BackendError> {
        self.adapters[adapter]
            .accounts
            .clone()
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    fn profile_info(
        &mut self,
        adapter: &String,
        account_id: &str,
    ) -> Result<Option<PlatformProfileInfo>, BackendError> {
        self.profile_calls.push(account_id.into());
        (self.adapters[adapter].profile.expect("profile"))(account_id)
    }

    fn load_warning_states(
        &mut self,
        adapter: &String,
        accounts: &[PlatformAccount],
        options: WarningLoadOptions,
    ) -> Result<WarningLoad, BackendError> {
        self.warning_calls
            .push((accounts.iter().map(|a| a.id.clone()).collect(), options));
        Ok(self.adapters[adapter].warnings.clone().expect("warnings"))
    }

    fn no_accounts_toast(&self, adapter: &String) -> Option<Text> {
        self.adapters[adapter].no_accounts.clone()
    }
}

fn def(id: &str, refresh: Option<ProfileRefresh>) -> PlatformDef {
    PlatformDef {
        id: id.into(),
        name: id.into(),
        accent: "#000000".into(),
        implemented: true,
        supported_os: Vec::new(),
        settings_tab_key: None,
        path_label_key: None,
        path_placeholder: None,
        capabilities: PlatformCapabilities {
            profile_refresh: refresh,
            ..PlatformCapabilities::default()
        },
        user_provided: false,
    }
}

fn defs() -> Vec<PlatformDef> {
    vec![
        def(
            "steam",
            Some(ProfileRefresh {
                avatars: true,
                bans: true,
            }),
        ),
        def("riot", None),
        def(
            "epic",
            Some(ProfileRefresh {
                avatars: true,
                bans: false,
            }),
        ),
    ]
}

fn accounts() -> Vec<PlatformAccount> {
    ["a1", "a2"]
        .into_iter()
        .map(|id| PlatformAccount {
            id: id.into(),
            display_name: String::new(),
            username: String::new(),
            last_login_at_sec: None,
        })
        .collect()
}

fn avatar(_id: &str) -> Result<Option<PlatformProfileInfo>, BackendError> {
    Ok(Some(PlatformProfileInfo {
        avatar_url: Some("https://cdn.example/a.png".into()),
        ..PlatformProfileInfo::default()
    }))
}

fn steam_with(adapter: FakeAdapter) -> Env {
    let mut env = Env::default();
    env.adapters.insert("steam".into(), adapter);
    env
}

fn settled(outcome: WarningRefreshOutcome) -> WarningLoad {
    WarningLoad {
        outcome: Some(outcome),
        ..WarningLoad::default()
    }
}

fn avatar_reload() -> Option<LoadOptions> {
    Some(LoadOptions {
        silent: true,
        force_refresh: true,
        ..LoadOptions::default()
    })
}

fn toast(text: Text, kind: ToastKind) -> Toast {
    Toast::new(text, kind)
}

#[test]
fn refreshes_avatars_only_on_platforms_that_declare_it_reloading_the_active_tab() {
    let mut env = steam_with(FakeAdapter {
        accounts: Some(Ok(accounts())),
        profile: Some(avatar),
        ..FakeAdapter::default()
    });
    env.adapters.insert("epic".into(), FakeAdapter::default());

    let report = refresh_avatars_now(&defs(), "steam", &mut env);

    assert_eq!(env.ensured, ["steam", "epic"]);
    assert_eq!(env.profile_calls, ["a1", "a2"]);
    assert_eq!(report.reload_active, avatar_reload());
    // Epic has no profile fetch: skipped before loading anything.
    assert_eq!(
        report.toasts,
        [toast(
            Text::with("toast.avatarRefreshComplete", [("count", 2.0)]),
            ToastKind::Success
        )]
    );
}

#[test]
fn refreshes_bans_with_a_forced_load_and_does_not_reload_another_tab() {
    let mut env = steam_with(FakeAdapter {
        accounts: Some(Ok(accounts())),
        warnings: Some(WarningLoad::default()),
        ..FakeAdapter::default()
    });

    let report = refresh_bans_now(&defs(), "riot", &mut env);

    assert_eq!(
        env.warning_calls,
        [(
            vec!["a1".to_string(), "a2".to_string()],
            WarningLoadOptions {
                force_refresh: true,
                silent: false
            }
        )]
    );
    assert!(report.reload_active.is_none());
    assert_eq!(
        report.toasts,
        [toast(
            Text::with("toast.banRefreshComplete", [("count", 2.0)]),
            ToastKind::Success
        )]
    );
}

#[test]
fn reports_a_failed_avatar_refresh_when_every_profile_is_missing_or_rejected() {
    let failed = [toast(Text::key("toast.refreshFailed"), ToastKind::Error)];

    let mut missing = steam_with(FakeAdapter {
        accounts: Some(Ok(accounts())),
        profile: Some(|_| Ok(None)),
        ..FakeAdapter::default()
    });
    assert_eq!(
        refresh_avatars_now(&defs(), "steam", &mut missing).toasts,
        failed
    );

    let mut rejected = steam_with(FakeAdapter {
        accounts: Some(Ok(accounts())),
        profile: Some(|_| Err(BackendError::new("down"))),
        ..FakeAdapter::default()
    });
    assert_eq!(
        refresh_avatars_now(&defs(), "steam", &mut rejected).toasts,
        failed
    );
}

#[test]
fn reports_a_partial_avatar_refresh_when_only_some_profiles_return() {
    let mut env = steam_with(FakeAdapter {
        accounts: Some(Ok(accounts())),
        profile: Some(|id| if id == "a1" { avatar(id) } else { Ok(None) }),
        ..FakeAdapter::default()
    });

    let report = refresh_avatars_now(&defs(), "steam", &mut env);

    assert_eq!(report.reload_active, avatar_reload());
    assert_eq!(
        report.toasts,
        [toast(
            Text::with("toast.refreshPartial", [("ok", 1.0), ("count", 2.0)]),
            ToastKind::Info
        )]
    );
}

fn bans_settling(outcome: WarningRefreshOutcome) -> Env {
    steam_with(FakeAdapter {
        accounts: Some(Ok(accounts())),
        warnings: Some(settled(outcome)),
        ..FakeAdapter::default()
    })
}

#[test]
fn does_not_announce_a_finished_ban_refresh_when_the_check_failed() {
    let mut env = bans_settling(WarningRefreshOutcome::Failed);
    assert!(refresh_bans_now(&defs(), "riot", &mut env)
        .toasts
        .is_empty());
}

#[test]
fn says_the_ban_refresh_was_skipped_when_no_api_key_is_set() {
    let mut env = bans_settling(WarningRefreshOutcome::NoApiKey);

    let report = refresh_bans_now(&defs(), "steam", &mut env);

    assert!(report.reload_active.is_none());
    assert_eq!(
        report.toasts,
        [toast(
            Text::key("toast.banRefreshNoApiKey"),
            ToastKind::Error
        )]
    );
}

#[test]
fn reports_a_partial_ban_refresh_when_steam_left_accounts_out() {
    let mut env = bans_settling(WarningRefreshOutcome::Partial {
        checked: 1,
        requested: 2,
    });

    let report = refresh_bans_now(&defs(), "riot", &mut env);

    assert_eq!(
        report.toasts,
        [toast(
            Text::with("toast.refreshPartial", [("ok", 1.0), ("count", 2.0)]),
            ToastKind::Info
        )]
    );
}

#[test]
fn shows_the_platforms_empty_message_and_an_error_toast_when_a_step_throws() {
    let mut empty = steam_with(FakeAdapter {
        accounts: Some(Ok(Vec::new())),
        profile: Some(|_| Ok(None)),
        warnings: Some(WarningLoad::default()),
        no_accounts: Some(Text::raw("no accounts")),
    });
    assert_eq!(
        refresh_avatars_now(&defs(), "steam", &mut empty).toasts,
        [toast(Text::raw("no accounts"), ToastKind::Default)]
    );

    let mut failing = steam_with(FakeAdapter {
        accounts: Some(Err(BackendError::new("boom"))),
        warnings: Some(WarningLoad::default()),
        ..FakeAdapter::default()
    });
    assert_eq!(
        refresh_bans_now(&defs(), "steam", &mut failing).toasts,
        [toast(Text::key("toast.refreshFailed"), ToastKind::Error)]
    );
}
