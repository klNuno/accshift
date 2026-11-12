//! Deep links: the launch arguments, a second launch, the `accshift://`
//! scheme registration ([`registration`]), the link parser ([`link`]) and
//! the controller that walks a link to an account switch ([`controller`]).

mod controller;
mod link;
mod registration;

pub use controller::{
    DeepLinkAction, DeepLinkAnswer, DeepLinkController, DeepLinkView, BOOT_TIMEOUT, LOAD_TIMEOUT,
};
pub use link::{
    decode_uri_component, find_account, parse_deep_link, DeepLinkSwitchRequest,
    MAX_DEEP_LINK_URL_LENGTH,
};
#[cfg(target_os = "linux")]
pub use registration::register_linux;
#[cfg(windows)]
pub use registration::WindowsRegistry;
pub use registration::{
    current_exe_for_registration, desktop_template, is_registered, register_all, register_scheme,
    registered_command, registration_entries, schemes_need_registration, wire_scheme_registration,
    RegistrationFailure, RegistryValue, SchemeRegistry,
};

use url::Url;

/// The URL schemes the app answers, as in `plugins.deep-link.desktop.schemes`
/// of tauri.conf.json.
pub const SCHEMES: &[&str] = &["accshift"];

/// The link a launch carries, Windows and Linux: the argument list must be
/// the executable plus exactly one argument, a URL whose scheme is
/// configured. macOS gets URLs from the OS instead.
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

/// What a second launch does in the running app: its link, if any, goes to
/// the deep-link listener, a log line is written and the window shows.
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

/// What the app does with delivered URLs.
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

/// The log form of a link: scheme, action and at most 32 characters of the
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
