//! Riot games: the closed list of products accshift can launch after a
//! switch, which of them are installed, and the switch that ends on a game
//! instead of the bare client.

#[allow(unused_imports)]
use super::*;

/// The product the plain switch starts.
pub(super) const RIOT_CLIENT_PRODUCT: &str = "riot-client";

/// The only patchline offered: PBE and other patchlines are left out.
pub(super) const RIOT_LIVE_PATCHLINE: &str = "live";

/// Products accshift may start, `(product id, display name)`, in menu order.
/// The id is the `--launch-product` value and the `Metadata/<id>.live` folder
/// name. A closed list: no free command argument reaches the client.
pub(super) const RIOT_GAME_PRODUCTS: &[(&str, &str)] = &[
    ("valorant", "VALORANT"),
    ("league_of_legends", "League of Legends"),
    ("bacon", "Legends of Runeterra"),
];

/// An installed Riot game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotGame {
    pub product_id: String,
    pub patchline: String,
    pub name: String,
}

/// The display name of a known product, `None` for anything else.
pub(super) fn known_product(product_id: &str) -> Option<&'static str> {
    RIOT_GAME_PRODUCTS
        .iter()
        .find(|(id, _)| *id == product_id)
        .map(|(_, name)| *name)
}

/// Riot Games `Metadata` directory (`%PROGRAMDATA%\Riot Games\Metadata` on
/// Windows, `/Users/Shared/Riot Games/Metadata` on macOS), where the Riot
/// Client keeps one `<product>.<patchline>` folder per installed product.
pub(super) fn metadata_root() -> Option<PathBuf> {
    riot_shared_data_root()
        .ok()
        .map(|root| riot_metadata_root_in(&root))
}

/// The `product_install_full_path` of a `product_settings.yaml` body, quotes
/// and comments stripped, `None` when absent or empty.
pub(super) fn product_install_path(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        if top_level_key(line)? != "product_install_full_path" {
            return None;
        }
        let (_, value) = line.split_once(':')?;
        let value = value.trim();
        let value = match value.chars().next() {
            Some(quote @ ('"' | '\'')) => value[1..].split(quote).next().unwrap_or_default(),
            _ => value.split(" #").next().unwrap_or_default().trim(),
        };
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// The known products installed under `metadata_root`, in menu order: a
/// product counts when its live `product_settings.yaml` names an install
/// folder that exists.
pub(super) fn installed_games_in(metadata_root: &Path) -> Vec<RiotGame> {
    RIOT_GAME_PRODUCTS
        .iter()
        .filter(|(id, _)| {
            let folder = format!("{id}.{RIOT_LIVE_PATCHLINE}");
            let settings = metadata_root
                .join(&folder)
                .join(format!("{folder}.product_settings.yaml"));
            fs::read_to_string(settings)
                .ok()
                .and_then(|contents| product_install_path(&contents))
                .is_some_and(|path| Path::new(&path).is_dir())
        })
        .map(|(id, name)| RiotGame {
            product_id: (*id).to_string(),
            patchline: RIOT_LIVE_PATCHLINE.to_string(),
            name: (*name).to_string(),
        })
        .collect()
}

/// The installed Riot games of this machine, empty when the Riot Client
/// metadata cannot be read.
pub fn get_installed_games() -> Vec<RiotGame> {
    metadata_root()
        .map(|root| installed_games_in(&root))
        .unwrap_or_default()
}

/// The arguments that make `RiotClientServices.exe` start `product_id`.
pub(super) fn launch_args(product_id: &str) -> [String; 2] {
    [
        format!("--launch-product={product_id}"),
        format!("--launch-patchline={RIOT_LIVE_PATCHLINE}"),
    ]
}

/// What starts once the session work is done: the game after a switch that
/// worked, the bare client after one that failed, so a game never opens on
/// the wrong account.
pub(super) fn product_after_switch(work_ok: bool, product_id: &str) -> &str {
    if work_ok {
        product_id
    } else {
        RIOT_CLIENT_PRODUCT
    }
}

/// True when the switch can be skipped: the target is the current profile,
/// the client runs and its signed-in account is the profile's.
pub(super) fn already_signed_in(
    cfg: &config::AppConfig,
    target_id: &str,
    client_running: bool,
    live: Option<&RiotDetectedIdentity>,
) -> bool {
    client_running
        && cfg.riot.current_profile_id == target_id
        && find_profile(cfg, target_id)
            .is_some_and(|profile| check_live_identity(profile, live) == IdentityCheck::Match)
}

/// Refuses the launch before any process work: an unknown product, a
/// profile that does not exist, then a product `installed` does not list
/// (read only once the cheaper checks passed).
pub(super) fn check_launch(
    cfg: &config::AppConfig,
    target_id: &str,
    product_id: &str,
    installed: impl FnOnce() -> Vec<RiotGame>,
) -> Result<(), String> {
    let Some(name) = known_product(product_id) else {
        return Err(format!("Unknown Riot game: {product_id}"));
    };
    if find_profile(cfg, target_id).is_none() {
        return Err("Riot profile not found".into());
    }
    if !installed().iter().any(|game| game.product_id == product_id) {
        return Err(format!("{name} is not installed"));
    }
    Ok(())
}

const LAUNCH_SOURCE: &str = "riot.switch_profile_and_launch_game";

/// Switches to `profile_id`, then starts `product_id` instead of the bare
/// client. The checks that need no process run first: the profile id, the
/// product, the profile and the install.
pub fn switch_profile_and_launch_game(
    app_handle: AppCtx,
    profile_id: String,
    product_id: String,
) -> Result<(), String> {
    let target_id = normalize_profile_id(&profile_id)?;
    let cfg = config::load_config(&app_handle);
    check_launch(&cfg, &target_id, &product_id, get_installed_games)?;

    let client_running = is_any_process_running(RIOT_CLIENT_PROCESS_NAMES);
    let live = if client_running {
        detect_live_identity().ok()
    } else {
        None
    };
    if already_signed_in(&cfg, &target_id, client_running, live.as_ref()) {
        let client_path = resolve_riot_client_path(&app_handle)?;
        return match launch_riot_product(&client_path, &product_id) {
            Ok(()) => {
                log_platform_info(
                    &app_handle,
                    LAUNCH_SOURCE,
                    "Target profile already signed in; launched game without switching",
                    format!("product={product_id}"),
                );
                Ok(())
            }
            Err(error) => {
                log_platform_error(
                    &app_handle,
                    LAUNCH_SOURCE,
                    "Riot game launch failed",
                    format!("product={product_id}; error={error}"),
                );
                Err(error)
            }
        };
    }

    run_switch(&app_handle, &target_id, LAUNCH_SOURCE, &product_id)
}
