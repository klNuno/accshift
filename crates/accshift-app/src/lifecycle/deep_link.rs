//! Deep links: the `tauri-plugin-deep-link` 2.4.10 desktop half (launch
//! arguments, scheme registration), the Rust handler of
//! `src-tauri/src/boot.rs` (`wire_deep_links`, the log shape) and the TS
//! side (`src/lib/app/deepLinkUrl.ts`, `src/lib/app/useDeepLink.svelte.ts`).
//!
//! Plugin lines are in its `src/lib.rs`.

use crate::accounts::loading::is_platform_usable;
use crate::js;
use crate::platforms::{PlatformAccount, PlatformRegistry, RuntimeOs};
use crate::settings::AppSettings;
use crate::text::Text;
use std::time::{Duration, Instant};
use url::Url;

/// `plugins.deep-link.desktop.schemes` of tauri.conf.json.
pub const SCHEMES: &[&str] = &["accshift"];

/// `handle_cli_arguments` (lines 196-222), Windows and Linux: the argument
/// list must be the executable plus exactly one argument, a URL whose
/// scheme is configured. That URL becomes the plugin's `current` and is
/// emitted as `deep-link://new-url`. macOS gets URLs from the OS instead.
pub fn handle_cli_arguments<S: AsRef<str>>(args: &[S], schemes: &[&str]) -> Option<Url> {
    if !(cfg!(windows) || cfg!(target_os = "linux")) {
        return None;
    }
    if args.len() != 2 {
        return None;
    }
    let url = args[1].as_ref().parse::<Url>().ok()?;
    schemes.contains(&url.scheme()).then_some(url)
}

/// What a second launch does in the running app: the single-instance
/// callback of src-tauri/src/lib.rs (lines 82-97), with the deep-link
/// plugin's `handle_cli_arguments` the `deep-link` feature runs first.
#[derive(Debug, Clone, PartialEq)]
pub struct SecondLaunch {
    /// The URL delivered to the deep-link listeners, if the arguments were one.
    pub url: Option<Url>,
    /// `backend.single-instance` info line.
    pub log_source: &'static str,
    pub log_message: &'static str,
    /// Show and focus the main window: only once boot completed.
    pub show_window: bool,
}

pub fn second_launch<S: AsRef<str>>(args: &[S], boot_completed: bool) -> SecondLaunch {
    SecondLaunch {
        url: handle_cli_arguments(args, SCHEMES),
        log_source: "backend.single-instance",
        log_message: "Second instance launch redirected to the running app",
        show_window: boot_completed,
    }
}

/// What `on_open_url` of boot.rs (lines 699-731) does with delivered URLs.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenUrlEffects {
    /// `info`, `backend.deep-link`, "Deep link received", these details.
    pub log_details: String,
    /// `telemetry::Event::DeepLinkUsed` (`RealBackend::track_deep_link`).
    pub track: bool,
    pub show_window: bool,
}

pub fn on_open_url(urls: &[Url], boot_completed: bool) -> OpenUrlEffects {
    OpenUrlEffects {
        log_details: urls
            .iter()
            .map(deep_link_log_shape)
            .collect::<Vec<_>>()
            .join(" "),
        track: true,
        show_window: boot_completed,
    }
}

/// boot.rs lines 147-161: scheme, action and at most 32 characters of the
/// platform; the account and any query or fragment are replaced.
pub fn deep_link_log_shape(url: &Url) -> String {
    let mut segments = url.path().split('/').filter(|s| !s.is_empty());
    let mut shape = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    if let Some(platform) = segments.next() {
        shape.push('/');
        shape.push_str(&platform.chars().take(32).collect::<String>());
    }
    if segments.next().is_some() {
        shape.push_str("/<account>");
    }
    if segments.next().is_some() || url.query().is_some() || url.fragment().is_some() {
        shape.push_str("<+extra>");
    }
    shape
}

/// boot.rs lines 652-654: register unless every scheme answered `true`. A
/// failed check must arrive as `false` (fail open).
pub fn schemes_need_registration(schemes: &[String], registered: &[bool]) -> bool {
    schemes.len() != registered.len() || registered.iter().any(|r| !r)
}

/// `dunce::simplified(current_exe())`, what the registry entries name.
/// tauri-utils canonicalizes the path, which adds `\\?\` on Windows; dunce
/// takes it off again.
pub fn current_exe_for_registration() -> std::io::Result<String> {
    let exe = std::fs::canonicalize(std::env::current_exe()?)?;
    Ok(dunce::simplified(&exe).display().to_string())
}

/// One registry value write of [`registration_entries`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryValue {
    /// Under `HKEY_CURRENT_USER`.
    pub key: String,
    /// `""` is the default value.
    pub name: String,
    pub value: String,
}

/// `register` on Windows (lines 259-281), as data: the four values written
/// under `HKCU\Software\Classes\{scheme}`.
pub fn registration_entries(scheme: &str, identifier: &str, exe: &str) -> Vec<RegistryValue> {
    let key_base = format!("Software\\Classes\\{scheme}");
    let value = |key: String, name: &str, value: String| RegistryValue {
        key,
        name: name.to_string(),
        value,
    };
    vec![
        value(key_base.clone(), "", format!("URL:{identifier} protocol")),
        value(key_base.clone(), "URL Protocol", String::new()),
        value(format!("{key_base}\\DefaultIcon"), "", format!("{exe},0")),
        value(
            format!("{key_base}\\shell\\open\\command"),
            "",
            format!("\"{exe}\" \"%1\""),
        ),
    ]
}

/// The command `is_registered` compares with (lines 432-448).
pub fn registered_command(exe: &str) -> String {
    format!("\"{exe}\" \"%1\"")
}

/// The registry, behind a trait so tests never touch the real one.
pub trait SchemeRegistry {
    /// Create `HKCU\{key}` and set one string value.
    fn set_string(&mut self, key: &str, name: &str, value: &str) -> Result<(), String>;
    /// `HKEY_CLASSES_ROOT\{key}` default value; `Ok(None)` when the key does
    /// not open.
    fn classes_root_default(&self, key: &str) -> Result<Option<String>, String>;
}

/// `register` for one scheme.
pub fn register_scheme(
    registry: &mut dyn SchemeRegistry,
    scheme: &str,
    identifier: &str,
    exe: &str,
) -> Result<(), String> {
    for entry in registration_entries(scheme, identifier, exe) {
        registry.set_string(&entry.key, &entry.name, &entry.value)?;
    }
    Ok(())
}

/// `register_all`: every configured scheme, stopping at the first error.
pub fn register_all(
    registry: &mut dyn SchemeRegistry,
    schemes: &[&str],
    identifier: &str,
    exe: &str,
) -> Result<(), String> {
    for scheme in schemes {
        register_scheme(registry, scheme, identifier, exe)?;
    }
    Ok(())
}

/// `is_registered` on Windows (lines 432-448): the merged class view's
/// `{scheme}\shell\open\command` must be exactly `"{exe}" "%1"`.
pub fn is_registered(
    registry: &dyn SchemeRegistry,
    scheme: &str,
    exe: &str,
) -> Result<bool, String> {
    match registry.classes_root_default(&format!("{scheme}\\shell\\open\\command"))? {
        None => Ok(false),
        Some(command) => Ok(command == registered_command(exe)),
    }
}

/// The real registry, on `windows-registry` like the plugin.
#[cfg(windows)]
pub struct WindowsRegistry;

#[cfg(windows)]
impl SchemeRegistry for WindowsRegistry {
    fn set_string(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
        windows_registry::CURRENT_USER
            .create(key)
            .and_then(|key| key.set_string(name, value))
            .map_err(|e| e.to_string())
    }

    fn classes_root_default(&self, key: &str) -> Result<Option<String>, String> {
        let Ok(key) = windows_registry::CLASSES_ROOT.open(key) else {
            return Ok(None);
        };
        key.get_string("").map(Some).map_err(|e| e.to_string())
    }
}

/// The warn line `wire_deep_links` writes when registration fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationFailure {
    pub source: &'static str,
    pub message: &'static str,
    pub details: String,
}

/// boot.rs lines 661-695 on Windows: read each scheme, register them all
/// only when one does not point at this executable.
pub fn wire_scheme_registration(
    registry: &mut dyn SchemeRegistry,
    schemes: &[&str],
    identifier: &str,
    exe: &str,
) -> Result<bool, RegistrationFailure> {
    let owned: Vec<String> = schemes.iter().map(ToString::to_string).collect();
    let registered: Vec<bool> = schemes
        .iter()
        .map(|scheme| is_registered(registry, scheme, exe).unwrap_or(false))
        .collect();
    if !schemes_need_registration(&owned, &registered) {
        return Ok(false);
    }
    register_all(registry, schemes, identifier, exe)
        .map(|()| true)
        .map_err(|details| RegistrationFailure {
            source: "backend.deep-link",
            message: "Failed to register accshift:// scheme",
            details,
        })
}

/// `register` on Linux (lines 283-364): a `{bin}-handler.desktop` file in
/// `{data_dir}/applications` claiming `x-scheme-handler/{scheme}`, merged
/// into an existing file, then `update-desktop-database` and `xdg-mime
/// default`. `wire_deep_links` runs it on every launch.
#[cfg(target_os = "linux")]
pub fn register_linux(scheme: &str, product_name: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::Command;
    let bin = std::fs::canonicalize(std::env::current_exe().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let file_name = format!(
        "{}-handler.desktop",
        bin.file_name().unwrap_or_default().to_string_lossy()
    );
    let exec = std::env::var_os("APPIMAGE")
        .unwrap_or_else(|| bin.clone().into_os_string())
        .to_string_lossy()
        .to_string();
    let qualified_exec = format!("\"{exec}\" %u");
    let target = dirs::data_dir()
        .ok_or_else(|| "unknown data directory".to_string())?
        .join("applications");
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let target_file = target.join(&file_name);
    let mime_type = format!("x-scheme-handler/{scheme}");

    if let Ok(mut desktop_file) = ini::Ini::load_from_file(&target_file) {
        if let Some(section) = desktop_file.section_mut(Some("Desktop Entry")) {
            let old_mimes = section.remove("MimeType").unwrap_or_default();
            let mut change = false;
            if !old_mimes.split(';').any(|mime| mime == mime_type) {
                section.append("MimeType", format!("{mime_type};{old_mimes}"));
                change = true;
            } else {
                section.insert("MimeType".to_string(), old_mimes);
            }
            let old_exec = section.remove("Exec").unwrap_or_default();
            if old_exec != qualified_exec {
                section.append("Exec", qualified_exec);
                change = true;
            } else {
                section.insert("Exec".to_string(), old_exec);
            }
            if change {
                desktop_file
                    .write_to_file(&target_file)
                    .map_err(|e| e.to_string())?;
            }
        }
    } else {
        let mut file = std::fs::File::create(&target_file).map_err(|e| e.to_string())?;
        file.write_all(desktop_template(product_name, &qualified_exec, &mime_type).as_bytes())
            .map_err(|e| e.to_string())?;
    }
    Command::new("update-desktop-database")
        .arg(&target)
        .status()
        .map_err(|e| e.to_string())?;
    Command::new("xdg-mime")
        .args(["default", &file_name, mime_type.as_str()])
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// The plugin's `template.desktop`.
pub fn desktop_template(name: &str, qualified_exec: &str, mime_type: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={qualified_exec}\nTerminal=false\nMimeType={mime_type}\nNoDisplay=true\n"
    )
}

/// `DeepLinkSwitchRequest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepLinkSwitchRequest {
    pub platform_id: String,
    pub account_ref: String,
}

/// `MAX_DEEP_LINK_URL_LENGTH`, in UTF-16 units.
pub const MAX_DEEP_LINK_URL_LENGTH: usize = 2048;

/// `parseDeepLink`: `accshift://switch/<platformId>/<accountRef>`.
pub fn parse_deep_link(raw_url: &str) -> Option<DeepLinkSwitchRequest> {
    let trimmed = js::trim(raw_url);
    if js::utf16_len(trimmed) > MAX_DEEP_LINK_URL_LENGTH {
        return None;
    }
    const PREFIX: &str = "accshift://";
    let head = trimmed.get(..PREFIX.len())?;
    if !head.eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let rest = &trimmed[PREFIX.len()..];
    let path = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let segments: Vec<String> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| decode_uri_component(segment).unwrap_or_else(|| segment.to_string()))
        .collect();
    if segments.len() == 3 && segments[0].to_lowercase() == "switch" {
        let platform_id = segments[1].to_lowercase();
        let account_ref = js::trim(&segments[2]).to_string();
        if !platform_id.is_empty() && !account_ref.is_empty() {
            return Some(DeepLinkSwitchRequest {
                platform_id,
                account_ref,
            });
        }
    }
    None
}

/// `decodeURIComponent`: `None` where JS throws (a `%` without two hex
/// digits, or bytes that are not UTF-8).
pub fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `findAccount`: an id match wins; a username or display name must be
/// unique, case-insensitive and trimmed.
pub fn find_account<'a>(
    accounts: &'a [PlatformAccount],
    reference: &str,
) -> Option<&'a PlatformAccount> {
    let needle = js::trim(reference).to_lowercase();
    let key = |value: &str| js::trim(value).to_lowercase();
    if let Some(by_id) = accounts.iter().find(|account| key(&account.id) == needle) {
        return Some(by_id);
    }
    let by_username: Vec<_> = accounts
        .iter()
        .filter(|account| key(&account.username) == needle)
        .collect();
    match by_username.len() {
        1 => return Some(by_username[0]),
        n if n > 1 => return None,
        _ => {}
    }
    let by_display: Vec<_> = accounts
        .iter()
        .filter(|account| key(&account.display_name) == needle)
        .collect();
    (by_display.len() == 1).then(|| by_display[0])
}

/// `BOOT_TIMEOUT_MS`.
pub const BOOT_TIMEOUT: Duration = Duration::from_millis(20_000);
/// `LOAD_TIMEOUT_MS`.
pub const LOAD_TIMEOUT: Duration = Duration::from_millis(15_000);

/// What the controller reads of the app on each [`DeepLinkController::poll`].
pub struct DeepLinkView<'a> {
    pub boot_ready: bool,
    pub pin_locked: bool,
    pub settings: &'a AppSettings,
    pub registry: &'a PlatformRegistry,
    pub runtime_os: RuntimeOs,
    pub active_tab: &'a str,
    pub loader_loading: bool,
    /// The visible tab's loaded accounts (`getAccounts`).
    pub accounts: &'a [PlatformAccount],
}

/// The work the controller hands the shell. Each one is answered with the
/// matching [`DeepLinkAnswer`].
#[derive(Debug, Clone, PartialEq)]
pub enum DeepLinkAction {
    Toast(Text),
    /// `loadPlatformAccounts(platformId)`: answer `PlatformAccounts`.
    LoadPlatformAccounts(String),
    /// `loadAccounts()` of the visible tab: answer `AccountsLoaded`.
    LoadAccounts,
    /// `confirmSwitch(account, platformName)`: answer `Confirmed`. A shell
    /// without the gate answers `true` at once.
    Confirm {
        account: PlatformAccount,
        platform_name: String,
    },
    /// `changeTab(platformId)`: answer `TabChanged`.
    ChangeTab(String),
    /// `switchToAccount(account)`: answer `Switched`.
    Switch(PlatformAccount),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeepLinkAnswer {
    PlatformAccounts(Vec<PlatformAccount>),
    AccountsLoaded,
    Confirmed(bool),
    TabChanged,
    Switched(bool),
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    /// `handleUrl`'s wait for boot (it goes on after the timeout).
    WaitBootForUrl {
        url: String,
        since: Instant,
    },
    /// `handleSwitch`'s own wait (it gives up after the timeout).
    WaitBootForSwitch {
        request: DeepLinkSwitchRequest,
        since: Instant,
    },
    AwaitPlatformAccounts(Switch),
    WaitLoader {
        switch: Switch,
        since: Instant,
    },
    AwaitReload(Switch),
    AwaitConfirm(Switch),
    AwaitTabChange(Switch),
    WaitLoaderAfterTab {
        switch: Switch,
        since: Instant,
    },
    AwaitReloadAfterTab(Switch),
    AwaitSwitch(Switch),
}

#[derive(Debug, Clone, PartialEq)]
struct Switch {
    request: DeepLinkSwitchRequest,
    platform_name: String,
    on_other_tab: bool,
    account: Option<PlatformAccount>,
}

/// `createDeepLinkController`: one link at a time, the latest one queued.
#[derive(Debug)]
pub struct DeepLinkController {
    started: bool,
    phase: Phase,
    pending: Option<String>,
}

impl Default for DeepLinkController {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepLinkController {
    pub fn new() -> Self {
        Self {
            started: false,
            phase: Phase::Idle,
            pending: None,
        }
    }

    /// `start`: listen from now on. URLs delivered before are not replayed:
    /// `onOpenUrl` only listens for `deep-link://new-url`.
    pub fn start(&mut self) {
        self.started = true;
    }

    /// `stop`: drop the listener. A link in flight finishes.
    pub fn stop(&mut self) {
        self.started = false;
    }

    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// The listener: each URL goes to `handleUrl`.
    pub fn on_urls<S: AsRef<str>>(&mut self, urls: &[S], now: Instant) {
        if !self.started {
            return;
        }
        for url in urls {
            self.handle_url(url.as_ref().to_string(), now);
        }
    }

    /// `handleUrl`: busy keeps only the latest link.
    fn handle_url(&mut self, url: String, now: Instant) {
        if self.is_busy() {
            self.pending = Some(url);
            return;
        }
        self.phase = Phase::WaitBootForUrl { url, since: now };
    }

    /// The `finally` of `handleUrl`.
    fn finish(&mut self, now: Instant) {
        self.phase = Phase::Idle;
        if let Some(next) = self.pending.take() {
            self.handle_url(next, now);
        }
    }

    /// Advances as far as the view allows; call once per frame while busy.
    pub fn poll(&mut self, view: &DeepLinkView<'_>, now: Instant) -> Vec<DeepLinkAction> {
        let mut actions = Vec::new();
        loop {
            let before = self.phase.clone();
            self.step(view, now, &mut actions);
            if self.phase == before {
                return actions;
            }
        }
    }

    fn step(&mut self, view: &DeepLinkView<'_>, now: Instant, actions: &mut Vec<DeepLinkAction>) {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => {}
            Phase::WaitBootForUrl { url, since } => {
                if !view.boot_ready && now.duration_since(since) <= BOOT_TIMEOUT {
                    self.phase = Phase::WaitBootForUrl { url, since };
                    return;
                }
                match parse_deep_link(&url) {
                    None => {
                        if view.settings.deep_links_enabled {
                            actions.push(DeepLinkAction::Toast(Text::key("toast.deepLinkInvalid")));
                        }
                        self.finish(now);
                    }
                    Some(request) => {
                        self.phase = Phase::WaitBootForSwitch {
                            request,
                            since: now,
                        };
                    }
                }
            }
            Phase::WaitBootForSwitch { request, since } => {
                if !view.boot_ready {
                    if now.duration_since(since) > BOOT_TIMEOUT {
                        self.finish(now);
                    } else {
                        self.phase = Phase::WaitBootForSwitch { request, since };
                    }
                    return;
                }
                self.gate(request, view, now, actions);
            }
            Phase::WaitLoader { switch, since } => {
                if view.loader_loading && now.duration_since(since) <= LOAD_TIMEOUT {
                    self.phase = Phase::WaitLoader { switch, since };
                    return;
                }
                match find_account(view.accounts, &switch.request.account_ref) {
                    Some(account) => self.found(switch, account.clone(), actions),
                    None => {
                        actions.push(DeepLinkAction::LoadAccounts);
                        self.phase = Phase::AwaitReload(switch);
                    }
                }
            }
            Phase::WaitLoaderAfterTab { switch, since } => {
                if view.loader_loading && now.duration_since(since) <= LOAD_TIMEOUT {
                    self.phase = Phase::WaitLoaderAfterTab { switch, since };
                    return;
                }
                let id = switch
                    .account
                    .as_ref()
                    .map(|a| a.id.clone())
                    .unwrap_or_default();
                match view.accounts.iter().find(|candidate| candidate.id == id) {
                    Some(account) => self.switch(switch, account.clone(), actions),
                    None => {
                        actions.push(DeepLinkAction::LoadAccounts);
                        self.phase = Phase::AwaitReloadAfterTab(switch);
                    }
                }
            }
            waiting => self.phase = waiting,
        }
    }

    /// The checks at the top of `handleSwitch`.
    fn gate(
        &mut self,
        request: DeepLinkSwitchRequest,
        view: &DeepLinkView<'_>,
        now: Instant,
        actions: &mut Vec<DeepLinkAction>,
    ) {
        if view.pin_locked {
            return self.finish(now);
        }
        let toast = |actions: &mut Vec<DeepLinkAction>, text: Text| {
            actions.push(DeepLinkAction::Toast(text));
        };
        if !view.settings.deep_links_enabled {
            toast(actions, Text::key("toast.deepLinkDisabled"));
            return self.finish(now);
        }
        let Some(def) = view.registry.definition(&request.platform_id) else {
            toast(
                actions,
                Text::with(
                    "toast.deepLinkUnknownPlatform",
                    [("platform", request.platform_id.clone())],
                ),
            );
            return self.finish(now);
        };
        if !is_platform_usable(view.registry, &request.platform_id, view.runtime_os) {
            toast(
                actions,
                Text::with(
                    "app.platformUnsupportedOs",
                    [("platform", def.name.clone())],
                ),
            );
            return self.finish(now);
        }
        if !view
            .settings
            .enabled_platforms
            .contains(&request.platform_id)
        {
            toast(
                actions,
                Text::with(
                    "toast.deepLinkPlatformDisabled",
                    [("platform", def.name.clone())],
                ),
            );
            return self.finish(now);
        }
        let on_other_tab = view.active_tab != request.platform_id;
        let switch = Switch {
            platform_name: def.name.clone(),
            on_other_tab,
            account: None,
            request,
        };
        if on_other_tab {
            actions.push(DeepLinkAction::LoadPlatformAccounts(
                switch.request.platform_id.clone(),
            ));
            self.phase = Phase::AwaitPlatformAccounts(switch);
        } else {
            self.phase = Phase::WaitLoader { switch, since: now };
        }
    }

    fn not_found(&mut self, switch: &Switch, actions: &mut Vec<DeepLinkAction>, now: Instant) {
        actions.push(DeepLinkAction::Toast(Text::with(
            "toast.deepLinkAccountNotFound",
            [
                ("platform", switch.platform_name.clone()),
                ("account", switch.request.account_ref.clone()),
            ],
        )));
        self.finish(now);
    }

    fn found(
        &mut self,
        mut switch: Switch,
        account: PlatformAccount,
        actions: &mut Vec<DeepLinkAction>,
    ) {
        actions.push(DeepLinkAction::Confirm {
            account: account.clone(),
            platform_name: switch.platform_name.clone(),
        });
        switch.account = Some(account);
        self.phase = Phase::AwaitConfirm(switch);
    }

    fn switch(
        &mut self,
        mut switch: Switch,
        account: PlatformAccount,
        actions: &mut Vec<DeepLinkAction>,
    ) {
        actions.push(DeepLinkAction::Switch(account.clone()));
        switch.account = Some(account);
        self.phase = Phase::AwaitSwitch(switch);
    }

    /// The shell's answer to the last action. `view` is read after it.
    pub fn answer(
        &mut self,
        answer: DeepLinkAnswer,
        view: &DeepLinkView<'_>,
        now: Instant,
    ) -> Vec<DeepLinkAction> {
        let mut actions = Vec::new();
        match (std::mem::replace(&mut self.phase, Phase::Idle), answer) {
            (Phase::AwaitPlatformAccounts(switch), DeepLinkAnswer::PlatformAccounts(accounts)) => {
                match find_account(&accounts, &switch.request.account_ref) {
                    Some(account) => self.found(switch, account.clone(), &mut actions),
                    None => self.not_found(&switch, &mut actions, now),
                }
            }
            (Phase::AwaitReload(switch), DeepLinkAnswer::AccountsLoaded) => {
                match find_account(view.accounts, &switch.request.account_ref) {
                    Some(account) => self.found(switch, account.clone(), &mut actions),
                    None => self.not_found(&switch, &mut actions, now),
                }
            }
            (Phase::AwaitConfirm(switch), DeepLinkAnswer::Confirmed(allowed)) => {
                if !allowed {
                    self.finish(now);
                } else if switch.on_other_tab || view.active_tab != switch.request.platform_id {
                    actions.push(DeepLinkAction::ChangeTab(
                        switch.request.platform_id.clone(),
                    ));
                    self.phase = Phase::AwaitTabChange(switch);
                } else if let Some(account) = switch.account.clone() {
                    self.switch(switch, account, &mut actions);
                } else {
                    self.finish(now);
                }
            }
            (Phase::AwaitTabChange(switch), DeepLinkAnswer::TabChanged) => {
                if view.active_tab != switch.request.platform_id {
                    self.finish(now);
                } else {
                    self.phase = Phase::WaitLoaderAfterTab { switch, since: now };
                }
            }
            (Phase::AwaitReloadAfterTab(switch), DeepLinkAnswer::AccountsLoaded) => {
                let id = switch
                    .account
                    .as_ref()
                    .map(|a| a.id.clone())
                    .unwrap_or_default();
                match view.accounts.iter().find(|candidate| candidate.id == id) {
                    Some(account) => self.switch(switch, account.clone(), &mut actions),
                    None => self.not_found(&switch, &mut actions, now),
                }
            }
            (Phase::AwaitSwitch(switch), DeepLinkAnswer::Switched(switched)) => {
                if switched {
                    let name = switch
                        .account
                        .as_ref()
                        .and_then(|account| {
                            [&account.display_name, &account.username, &account.id]
                                .into_iter()
                                .find(|value| !value.is_empty())
                                .cloned()
                        })
                        .unwrap_or_default();
                    actions.push(DeepLinkAction::Toast(Text::with(
                        "toast.deepLinkSwitched",
                        [("account", name)],
                    )));
                }
                self.finish(now);
            }
            (phase, _) => self.phase = phase,
        }
        actions.extend(self.poll(view, now));
        actions
    }
}
