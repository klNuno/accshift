//! Installed Riot games, launch arguments and the launch decisions. Synthetic
//! metadata in a scratch folder; nothing here starts a process.

use super::*;

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "accshift-riot-games-test-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// Writes `<metadata>/<folder>/<folder>.product_settings.yaml` naming `install`.
fn write_product(metadata: &Path, folder: &str, install: &Path) {
    let dir = metadata.join(folder);
    fs::create_dir_all(&dir).unwrap();
    let install = install.to_string_lossy().replace('\\', "/");
    fs::write(
        dir.join(format!("{folder}.product_settings.yaml")),
        format!(
            "product_install_full_path: \"{install}\"\nproduct_install_root: \"C:/Riot Games\"\nshould_repair: false\n"
        ),
    )
    .unwrap();
}

#[test]
fn product_install_path_reads_the_top_level_key_only() {
    assert_eq!(
        product_install_path("product_install_full_path: \"C:/Riot Games/VALORANT/live\"\n"),
        Some("C:/Riot Games/VALORANT/live".into())
    );
    assert_eq!(
        product_install_path("product_install_full_path: 'D:/Games/LoL'"),
        Some("D:/Games/LoL".into())
    );
    assert_eq!(
        product_install_path("product_install_full_path: D:/Games/LoL # moved\n"),
        Some("D:/Games/LoL".into())
    );
    assert_eq!(
        product_install_path("product_install_full_path: \"\"\n"),
        None
    );
    assert_eq!(
        product_install_path("nested:\n  product_install_full_path: \"C:/x\"\n"),
        None
    );
    assert_eq!(
        product_install_path("product_install_full_path_old: \"C:/x\"\n"),
        None
    );
}

#[test]
fn only_known_live_products_with_an_existing_install_are_installed() {
    let root = scratch("installed");
    let metadata = root.join("Metadata");
    let valorant = root.join("VALORANT").join("live");
    fs::create_dir_all(&valorant).unwrap();
    let unknown = root.join("Other");
    fs::create_dir_all(&unknown).unwrap();
    write_product(&metadata, "valorant.live", &valorant);
    // Metadata left behind by an uninstalled game: its folder is gone.
    write_product(
        &metadata,
        "league_of_legends.live",
        &root.join("League of Legends"),
    );
    // A product outside the closed list and a non-live patchline.
    write_product(&metadata, "lion.live", &unknown);
    write_product(&metadata, "valorant.pbe", &valorant);
    write_product(&metadata, "bacon.pbe", &unknown);

    let games = installed_games_in(&metadata);
    assert_eq!(
        games,
        vec![RiotGame {
            product_id: "valorant".into(),
            patchline: "live".into(),
            name: "VALORANT".into(),
        }]
    );

    // Installing League adds it after VALORANT, in the closed list's order.
    fs::create_dir_all(root.join("League of Legends")).unwrap();
    let ids: Vec<String> = installed_games_in(&metadata)
        .into_iter()
        .map(|game| game.product_id)
        .collect();
    assert_eq!(ids, ["valorant", "league_of_legends"]);

    assert!(installed_games_in(&root.join("missing")).is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn launch_args_name_the_product_on_the_live_patchline() {
    assert_eq!(
        launch_args("valorant"),
        [
            "--launch-product=valorant".to_string(),
            "--launch-patchline=live".to_string()
        ]
    );
    assert_eq!(
        launch_args(RIOT_CLIENT_PRODUCT),
        [
            "--launch-product=riot-client".to_string(),
            "--launch-patchline=live".to_string()
        ]
    );
}

#[test]
fn a_failed_switch_starts_the_bare_client_never_the_game() {
    assert_eq!(product_after_switch(true, "valorant"), "valorant");
    assert_eq!(product_after_switch(false, "valorant"), RIOT_CLIENT_PRODUCT);
    assert_eq!(
        product_after_switch(true, RIOT_CLIENT_PRODUCT),
        RIOT_CLIENT_PRODUCT
    );
}

fn config_with(current: &str, puuid: &str) -> config::AppConfig {
    let mut cfg = config::AppConfig::default();
    cfg.riot.profiles = vec![
        RiotProfileConfig {
            id: "alpha".into(),
            account_puuid: puuid.into(),
            snapshot_state: "ready".into(),
            ..Default::default()
        },
        RiotProfileConfig {
            id: "bravo".into(),
            account_puuid: "puuid-bravo".into(),
            snapshot_state: "ready".into(),
            ..Default::default()
        },
    ];
    cfg.riot.current_profile_id = current.into();
    cfg
}

fn live(puuid: &str) -> RiotDetectedIdentity {
    RiotDetectedIdentity {
        account_name: "name".into(),
        account_tag_line: "tag".into(),
        account_puuid: puuid.into(),
    }
}

#[test]
fn the_switch_is_skipped_only_when_the_running_client_holds_the_target() {
    let cfg = config_with("alpha", "puuid-alpha");
    let alpha = live("puuid-alpha");
    assert!(already_signed_in(&cfg, "alpha", true, Some(&alpha)));
    // Client closed, another live account, unknown live account.
    assert!(!already_signed_in(&cfg, "alpha", false, Some(&alpha)));
    assert!(!already_signed_in(
        &cfg,
        "alpha",
        true,
        Some(&live("puuid-bravo"))
    ));
    assert!(!already_signed_in(&cfg, "alpha", true, None));
    // Another target than the current profile.
    assert!(!already_signed_in(&cfg, "bravo", true, Some(&alpha)));
    // A profile without a puuid cannot prove the live account is its own.
    let unclaimed = config_with("alpha", "");
    assert!(!already_signed_in(&unclaimed, "alpha", true, Some(&alpha)));
}

#[test]
fn the_launch_is_refused_before_any_process_work() {
    let cfg = config_with("alpha", "puuid-alpha");
    let installed = || {
        vec![RiotGame {
            product_id: "valorant".into(),
            patchline: "live".into(),
            name: "VALORANT".into(),
        }]
    };
    assert_eq!(check_launch(&cfg, "alpha", "valorant", installed), Ok(()));
    assert_eq!(
        check_launch(&cfg, "alpha", "riot-client", installed),
        Err("Unknown Riot game: riot-client".into())
    );
    assert_eq!(
        check_launch(&cfg, "alpha", "valorant --x", installed),
        Err("Unknown Riot game: valorant --x".into())
    );
    assert_eq!(
        check_launch(&cfg, "charlie", "valorant", installed),
        Err("Riot profile not found".into())
    );
    assert_eq!(
        check_launch(&cfg, "alpha", "league_of_legends", installed),
        Err("League of Legends is not installed".into())
    );
    // The install scan is not read when a cheaper check already refused.
    let unread = || -> Vec<RiotGame> { panic!("install scan read") };
    assert!(check_launch(&cfg, "alpha", "nope", unread).is_err());
    assert!(check_launch(&cfg, "charlie", "valorant", unread).is_err());
}
