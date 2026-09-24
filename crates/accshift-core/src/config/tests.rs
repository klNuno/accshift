use super::*;
use std::sync::Arc;

struct TempCtx {
    root: std::path::PathBuf,
}

impl AppContext for TempCtx {
    fn app_config_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_local_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_cache_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.root.clone())
    }
}

fn tmp_ctx(tag: &str) -> Arc<TempCtx> {
    let root = std::env::temp_dir().join(format!(
        "accshift-config-test-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Arc::new(TempCtx { root })
}

#[test]
fn writer_waiting_for_file_lock_does_not_hold_config_mutex() {
    let _test_guard = config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("lock-order");
    save_config(&*ctx, &AppConfig::default()).unwrap();

    // Models a run_locked_blocking operation: this thread owns the file
    // lock before entering a nested config update.
    let outer =
        crate::lock::acquire_exclusive(&*ctx, std::time::Duration::from_millis(500)).unwrap();
    let writer_ctx = Arc::clone(&ctx);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        update_config(&*writer_ctx, |cfg| cfg.window_width = Some(1280.0))
    });

    started_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let mutex_available = !matches!(
        config_io_mutex().try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    );
    assert!(
        mutex_available,
        "a writer blocked on the file lock must not hold the config mutex"
    );

    drop(outer);
    assert!(writer.join().unwrap().is_ok());
    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn save_refuses_to_overwrite_unreadable_local_config() {
    let _test_guard = config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("unreadable-local");

    // An existing local config holding the only copy of the Steam API key.
    let local_path = crate::storage::local_config_path(&*ctx).unwrap();
    std::fs::create_dir_all(local_path.parent().unwrap()).unwrap();
    let corrupt = b"{ this is not valid json";
    std::fs::write(&local_path, corrupt).unwrap();

    // A read of the existing-but-corrupt file must poison local writes.
    let _ = load_config(&*ctx);
    assert!(
        config_unreadable(&local_path),
        "corrupt existing local config should poison local writes"
    );

    // The next save must refuse to touch the local file so the secrets it
    // (still) holds are not wiped by the empty defaults the read produced.
    let cfg = AppConfig::default();
    let result = save_config(&*ctx, &cfg);
    assert!(result.is_err(), "save must refuse while local is poisoned");

    let after = std::fs::read(&local_path).unwrap();
    assert_eq!(
        after, corrupt,
        "corrupt local config must be left byte-for-byte intact"
    );

    // Nothing is written while one half is poisoned, so the pair on disk
    // never mixes a new portable file with an old local one.
    let portable_path = crate::storage::portable_config_path(&*ctx).unwrap();
    assert!(
        !portable_path.exists(),
        "portable config must not be written when local is refused"
    );

    // A subsequent successful local read clears the poison flag, and the
    // following save is allowed again.
    let valid = local_config(&AppConfig {
        steam: SteamConfig {
            api_key: "kept-secret".into(),
            ..Default::default()
        },
        ..Default::default()
    });
    crate::storage::write_json_atomic(&local_path, &valid).unwrap();
    let loaded = load_config(&*ctx);
    assert!(
        !config_unreadable(&local_path),
        "successful local read should clear the poison flag"
    );
    assert_eq!(loaded.steam.api_key, "kept-secret");
    assert!(
        save_config(&*ctx, &loaded).is_ok(),
        "save should succeed once the local read recovers"
    );

    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn an_unreadable_portable_config_is_neither_cached_nor_overwritten() {
    let _test_guard = config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("unreadable-portable");
    let mut saved = AppConfig::default();
    saved.riot.profiles.push(RiotProfileConfig {
        id: "p1".into(),
        label: "Main".into(),
        ..Default::default()
    });
    save_config(&*ctx, &saved).unwrap();

    let portable_path = crate::storage::portable_config_path(&*ctx).unwrap();
    let corrupt = b"{ half a file";
    std::fs::write(&portable_path, corrupt).unwrap();

    let loaded = load_config(&*ctx);
    assert!(loaded.riot.profiles.is_empty());
    assert!(config_unreadable(&portable_path));
    assert!(
        update_config(&*ctx, |cfg| cfg.window_width = Some(900.0)).is_err(),
        "a save must be refused while portable is poisoned"
    );
    assert_eq!(std::fs::read(&portable_path).unwrap(), corrupt);

    // Once the file is readable again, the next load sees it: the
    // defaults built from the failed read were never cached.
    crate::storage::write_json_atomic(&portable_path, &portable_config(&saved)).unwrap();
    assert_eq!(load_config(&*ctx).riot.profiles.len(), 1);
    assert!(!config_unreadable(&portable_path));

    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn migrating_legacy_config_keeps_it_aside() {
    let _test_guard = config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("legacy-retire");
    let legacy_path = crate::storage::legacy_config_path(&*ctx).unwrap();
    std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    std::fs::write(&legacy_path, b"{}").unwrap();

    assert!(matches!(migrate_legacy_config(&*ctx), Some(Ok(()))));

    let mut retired = legacy_path.clone().into_os_string();
    retired.push(".migrated");
    assert!(!legacy_path.exists());
    assert_eq!(std::fs::read(retired).unwrap(), b"{}");

    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn save_window_size_preserves_other_config_fields() {
    let _test_guard = config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("window-size-update");
    let initial = AppConfig {
        steam: SteamConfig {
            path_override: "D:\\Games\\Steam".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    save_config(&*ctx, &initial).unwrap();

    save_window_size(&*ctx, 1280.0, 720.0).unwrap();

    let saved = load_config(&*ctx);
    assert_eq!(saved.steam.path_override, "D:\\Games\\Steam");
    assert_eq!(saved.window_width, Some(1280.0));
    assert_eq!(saved.window_height, Some(720.0));

    let _ = std::fs::remove_dir_all(&ctx.root);
}

// Regression for the launch-over-launch growth: the window reports a
// physical size, the builder consumes logical pixels, so a config that
// stored the physical number grew the window by the scale factor every
// time. The saver converts once, and the round trip is an identity at any
// scale.
#[test]
fn window_size_round_trips_in_logical_pixels_at_scale_1_5() {
    let _test_guard = config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("window-size-dpi");
    save_config(&*ctx, &AppConfig::default()).unwrap();

    let scale = 1.5_f64;
    let (logical_width, logical_height) = (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT);
    // What the window would report on a 150% display.
    let physical_width = logical_width * scale;
    let physical_height = logical_height * scale;

    save_window_geometry(
        &*ctx,
        logical_from_physical(physical_width, scale),
        logical_from_physical(physical_height, scale),
        None,
        None,
    )
    .unwrap();

    assert_eq!(
        load_window_size(&*ctx),
        Some((logical_width, logical_height)),
        "a saved size must come back unchanged, not scaled"
    );

    // Second launch: the restored size is what the window is built with, so
    // feeding it back through the same path must not move either.
    let (restored_width, restored_height) = load_window_size(&*ctx).unwrap();
    save_window_geometry(
        &*ctx,
        logical_from_physical(restored_width * scale, scale),
        logical_from_physical(restored_height * scale, scale),
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        load_window_size(&*ctx),
        Some((logical_width, logical_height))
    );

    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn window_size_is_clamped_to_something_openable() {
    assert_eq!(clamp_window_size(f64::NAN, 600.0), None);
    assert_eq!(clamp_window_size(0.0, 600.0), None);
    // A window collapsed to the minimum is a glitch, not a preference.
    assert_eq!(clamp_window_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT), None);
    assert_eq!(
        clamp_window_size(1.0e9, 1.0e9),
        Some((MAX_WINDOW_WIDTH, MAX_WINDOW_HEIGHT))
    );
    assert_eq!(
        clamp_window_size(200.0, 4000.0),
        Some((MIN_WINDOW_WIDTH, 4000.0))
    );
    assert_eq!(clamp_window_size(1280.0, 720.0), Some((1280.0, 720.0)));
}

#[test]
fn window_position_round_trips_and_survives_a_missing_field() {
    let _test_guard = config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("window-position");
    save_config(&*ctx, &AppConfig::default()).unwrap();

    // A config written before the field existed means "center me".
    assert_eq!(load_window_position(&*ctx), None);

    // A second monitor to the left gives a negative origin, which is valid.
    save_window_geometry(&*ctx, 1280.0, 720.0, Some((-1920.0, 240.0)), Some(1.5)).unwrap();
    assert_eq!(load_window_position(&*ctx), Some((-1920.0, 240.0)));
    assert_eq!(load_window_physical_position(&*ctx), Some((-2880, 360)));

    // A size-only save must not erase the placement.
    save_window_size(&*ctx, 1000.0, 600.0).unwrap();
    assert_eq!(load_window_position(&*ctx), Some((-1920.0, 240.0)));
    assert_eq!(load_window_size(&*ctx), Some((1000.0, 600.0)));

    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn a_nonsense_window_position_is_refused() {
    assert_eq!(clamp_window_position(f64::NAN, 0.0), None);
    assert_eq!(clamp_window_position(0.0, f64::INFINITY), None);
    assert_eq!(clamp_window_position(1.0e9, 0.0), None);
    assert_eq!(
        clamp_window_position(-1920.0, -80.0),
        Some((-1920.0, -80.0))
    );
}

#[test]
fn normalize_config_migrates_legacy_steam_fields() {
    let raw = RawAppConfig {
        steam: None,
        steam_api_key: "my-legacy-key".into(),
        steam_api_key_encrypted: "my-enc-key".into(),
        steam_path_override: "C:\\Steam".into(),
        window_width: Some(1024.0),
        window_height: Some(768.0),
        ..Default::default()
    };

    let cfg = normalize_config(raw);
    assert_eq!(cfg.steam.api_key, "my-legacy-key");
    assert_eq!(cfg.steam.api_key_encrypted, "my-enc-key");
    assert_eq!(cfg.steam.path_override, "C:\\Steam");
    assert_eq!(cfg.window_width, Some(1024.0));
    assert_eq!(cfg.window_height, Some(768.0));
}

#[test]
fn normalize_config_prefers_nested_steam_over_legacy() {
    let raw = RawAppConfig {
        steam: Some(SteamConfig {
            api_key: "nested-key".into(),
            api_key_encrypted: "nested-enc".into(),
            path_override: "D:\\Steam".into(),
            cs2_bridge: Cs2BridgeConfig::default(),
        }),
        steam_api_key: "legacy-key".into(),
        steam_api_key_encrypted: "legacy-enc".into(),
        steam_path_override: "C:\\Old".into(),
        ..Default::default()
    };

    let cfg = normalize_config(raw);
    assert_eq!(cfg.steam.api_key, "nested-key");
    assert_eq!(cfg.steam.api_key_encrypted, "nested-enc");
    assert_eq!(cfg.steam.path_override, "D:\\Steam");
}

#[test]
fn normalize_config_falls_back_to_legacy_when_nested_empty() {
    let raw = RawAppConfig {
        steam: Some(SteamConfig {
            api_key: String::new(),
            api_key_encrypted: String::new(),
            path_override: String::new(),
            cs2_bridge: Cs2BridgeConfig::default(),
        }),
        steam_api_key: "fallback-key".into(),
        steam_api_key_encrypted: "fallback-enc".into(),
        steam_path_override: "C:\\Fallback".into(),
        ..Default::default()
    };

    let cfg = normalize_config(raw);
    assert_eq!(cfg.steam.api_key, "fallback-key");
    assert_eq!(cfg.steam.api_key_encrypted, "fallback-enc");
    assert_eq!(cfg.steam.path_override, "C:\\Fallback");
}

#[test]
fn portable_config_strips_secrets_and_paths() {
    let config = AppConfig {
        steam: SteamConfig {
            api_key: "secret".into(),
            api_key_encrypted: "enc-secret".into(),
            path_override: "C:\\Steam".into(),
            cs2_bridge: Cs2BridgeConfig {
                enabled: true,
                url: "http://127.0.0.1:3000/api/bridge/accshift/key".into(),
                token_encrypted: "enc-token".into(),
            },
        },
        riot: RiotConfig {
            path_override: "/opt/riot".into(),
            profiles: vec![RiotProfileConfig {
                id: "p1".into(),
                label: "Main".into(),
                ..Default::default()
            }],
            current_profile_id: "p1".into(),
        },
        battle_net: BattleNetConfig {
            path_override: "C:\\BNet".into(),
            accounts: vec![],
        },
        ubisoft: UbisoftConfig {
            path_override: "C:\\Ubi".into(),
            accounts: vec![],
            forgotten_uuids: vec![],
            last_switch: None,
        },
        epic: EpicConfig {
            path_override: "C:\\Epic".into(),
            accounts: vec![],
        },
        gog: GogConfig {
            path_override: "C:\\GOG".into(),
            accounts: vec![],
        },
        jagex: JagexConfig {
            path_override: "C:\\Jagex".into(),
            accounts: vec![],
            current_account: String::new(),
        },
        discord: DiscordConfig::default(),
        roblox: RobloxConfig {
            accounts: vec![RobloxAccountConfig {
                user_id: "123".into(),
                username: "player1".into(),
                display_name: "Player".into(),
                cookie_encrypted: "cookie-secret".into(),
                last_used_at: Some(1000),
            }],
        },
        custom_platforms: Default::default(),
        telemetry: TelemetryConfig::default(),
        window_width: Some(1200.0),
        window_height: Some(800.0),
        window_x: Some(120.0),
        window_y: Some(64.0),
        window_scale: Some(1.25),
    };

    let p = portable_config(&config);

    // Secrets and paths stripped
    assert!(p.steam.api_key.is_empty());
    assert!(p.steam.api_key_encrypted.is_empty());
    assert!(p.steam.path_override.is_empty());
    assert!(!p.steam.cs2_bridge.enabled);
    assert!(p.steam.cs2_bridge.url.is_empty());
    assert!(p.steam.cs2_bridge.token_encrypted.is_empty());
    assert!(p.riot.path_override.is_empty());
    assert!(p.battle_net.path_override.is_empty());
    assert!(p.ubisoft.path_override.is_empty());
    assert!(p.epic.path_override.is_empty());
    assert!(p.gog.path_override.is_empty());
    assert!(p.jagex.path_override.is_empty());
    assert!(p.window_width.is_none());
    assert!(p.window_height.is_none());
    assert!(p.window_x.is_none());
    assert!(p.window_y.is_none());
    assert!(p.window_scale.is_none());

    // Roblox cookies stripped
    assert!(p.roblox.accounts[0].cookie_encrypted.is_empty());

    // Non-secret data preserved
    assert_eq!(p.riot.profiles.len(), 1);
    assert_eq!(p.riot.profiles[0].label, "Main");
    assert_eq!(p.roblox.accounts[0].username, "player1");
}

#[test]
fn local_config_keeps_only_secrets_paths_and_window() {
    let config = AppConfig {
        steam: SteamConfig {
            api_key: "secret".into(),
            api_key_encrypted: "enc".into(),
            path_override: "C:\\Steam".into(),
            cs2_bridge: Cs2BridgeConfig::default(),
        },
        riot: RiotConfig {
            path_override: "/opt/riot".into(),
            profiles: vec![RiotProfileConfig {
                id: "p1".into(),
                label: "Main".into(),
                ..Default::default()
            }],
            current_profile_id: "p1".into(),
        },
        battle_net: BattleNetConfig {
            path_override: "C:\\BNet".into(),
            accounts: vec![BattleNetAccountConfig {
                email: "test@example.com".into(),
                battle_tag: "Tag#1234".into(),
                last_used_at: None,
            }],
        },
        ubisoft: UbisoftConfig {
            path_override: "C:\\Ubi".into(),
            accounts: vec![],
            forgotten_uuids: vec![],
            last_switch: None,
        },
        epic: EpicConfig {
            path_override: "C:\\Epic".into(),
            accounts: vec![],
        },
        gog: GogConfig {
            path_override: "C:\\GOG".into(),
            accounts: vec![],
        },
        jagex: JagexConfig {
            path_override: "C:\\Jagex".into(),
            accounts: vec![],
            current_account: String::new(),
        },
        discord: DiscordConfig::default(),
        roblox: RobloxConfig {
            accounts: vec![RobloxAccountConfig {
                user_id: "456".into(),
                username: "player2".into(),
                display_name: "Player Two".into(),
                cookie_encrypted: "cookie-enc".into(),
                last_used_at: Some(2000),
            }],
        },
        custom_platforms: Default::default(),
        telemetry: TelemetryConfig::default(),
        window_width: Some(1024.0),
        window_height: Some(768.0),
        window_x: Some(-1920.0),
        window_y: Some(40.0),
        window_scale: Some(1.5),
    };

    let l = local_config(&config);

    // Secrets and paths kept
    assert_eq!(l.steam.api_key, "secret");
    assert_eq!(l.steam.api_key_encrypted, "enc");
    assert_eq!(l.steam.path_override, "C:\\Steam");
    assert_eq!(l.riot.path_override, "/opt/riot");
    assert_eq!(l.battle_net.path_override, "C:\\BNet");
    assert_eq!(l.ubisoft.path_override, "C:\\Ubi");
    assert_eq!(l.epic.path_override, "C:\\Epic");
    assert_eq!(l.gog.path_override, "C:\\GOG");
    assert_eq!(l.jagex.path_override, "C:\\Jagex");
    assert_eq!(l.window_width, Some(1024.0));
    assert_eq!(l.window_height, Some(768.0));
    assert_eq!(l.window_x, Some(-1920.0));
    assert_eq!(l.window_y, Some(40.0));
    assert_eq!(l.window_scale, Some(1.5));

    // Roblox local keeps user_id + cookie, but not username/display_name
    assert_eq!(l.roblox.accounts.len(), 1);
    assert_eq!(l.roblox.accounts[0].user_id, "456");
    assert_eq!(l.roblox.accounts[0].cookie_encrypted, "cookie-enc");
    assert!(l.roblox.accounts[0].username.is_empty());
    assert!(l.roblox.accounts[0].display_name.is_empty());

    // Non-secret account data not kept
    assert!(l.riot.profiles.is_empty());
    assert!(l.battle_net.accounts.is_empty());
}

#[test]
fn merge_split_configs_combines_portable_and_local() {
    let portable = AppConfig {
        steam: SteamConfig {
            api_key: String::new(),
            api_key_encrypted: String::new(),
            path_override: String::new(),
            cs2_bridge: Cs2BridgeConfig::default(),
        },
        riot: RiotConfig {
            path_override: String::new(),
            profiles: vec![RiotProfileConfig {
                id: "r1".into(),
                label: "Ranked".into(),
                ..Default::default()
            }],
            current_profile_id: "r1".into(),
        },
        roblox: RobloxConfig {
            accounts: vec![RobloxAccountConfig {
                user_id: "100".into(),
                username: "robloxer".into(),
                display_name: "Robloxer".into(),
                cookie_encrypted: String::new(),
                last_used_at: None,
            }],
        },
        ..Default::default()
    };

    let local = AppConfig {
        steam: SteamConfig {
            api_key: "local-key".into(),
            api_key_encrypted: "local-enc".into(),
            path_override: "C:\\LocalSteam".into(),
            cs2_bridge: Cs2BridgeConfig::default(),
        },
        riot: RiotConfig {
            path_override: "/local/riot".into(),
            ..Default::default()
        },
        battle_net: BattleNetConfig {
            path_override: "C:\\LocalBNet".into(),
            ..Default::default()
        },
        roblox: RobloxConfig {
            accounts: vec![RobloxAccountConfig {
                user_id: "100".into(),
                username: String::new(),
                display_name: String::new(),
                cookie_encrypted: "restored-cookie".into(),
                last_used_at: Some(5000),
            }],
        },
        window_width: Some(800.0),
        window_height: Some(600.0),
        ..Default::default()
    };

    let merged = merge_split_configs(portable, local);

    // Local secrets merged in
    assert_eq!(merged.steam.api_key, "local-key");
    assert_eq!(merged.steam.api_key_encrypted, "local-enc");
    assert_eq!(merged.steam.path_override, "C:\\LocalSteam");
    assert_eq!(merged.riot.path_override, "/local/riot");
    assert_eq!(merged.battle_net.path_override, "C:\\LocalBNet");

    // Portable data preserved
    assert_eq!(merged.riot.profiles.len(), 1);
    assert_eq!(merged.riot.profiles[0].label, "Ranked");

    // Window from local
    assert_eq!(merged.window_width, Some(800.0));
    assert_eq!(merged.window_height, Some(600.0));

    // Roblox: cookie and last_used_at merged into existing account
    assert_eq!(merged.roblox.accounts.len(), 1);
    assert_eq!(merged.roblox.accounts[0].username, "robloxer");
    assert_eq!(
        merged.roblox.accounts[0].cookie_encrypted,
        "restored-cookie"
    );
    assert_eq!(merged.roblox.accounts[0].last_used_at, Some(5000));
}

#[test]
fn merge_split_configs_adds_unknown_roblox_accounts_from_local() {
    let portable = AppConfig {
        roblox: RobloxConfig {
            accounts: vec![RobloxAccountConfig {
                user_id: "1".into(),
                username: "existing".into(),
                ..Default::default()
            }],
        },
        ..Default::default()
    };

    let local = AppConfig {
        roblox: RobloxConfig {
            accounts: vec![RobloxAccountConfig {
                user_id: "2".into(),
                cookie_encrypted: "new-cookie".into(),
                ..Default::default()
            }],
        },
        ..Default::default()
    };

    let merged = merge_split_configs(portable, local);
    assert_eq!(merged.roblox.accounts.len(), 2);
    assert_eq!(merged.roblox.accounts[1].user_id, "2");
    assert_eq!(merged.roblox.accounts[1].cookie_encrypted, "new-cookie");
}

#[test]
fn merge_split_configs_skips_local_roblox_with_empty_user_id() {
    let portable = AppConfig::default();
    let local = AppConfig {
        roblox: RobloxConfig {
            accounts: vec![RobloxAccountConfig {
                user_id: "  ".into(),
                cookie_encrypted: "orphan-cookie".into(),
                ..Default::default()
            }],
        },
        ..Default::default()
    };

    let merged = merge_split_configs(portable, local);
    assert!(merged.roblox.accounts.is_empty());
}

#[test]
fn local_config_skips_roblox_accounts_with_blank_user_id() {
    let config = AppConfig {
        roblox: RobloxConfig {
            accounts: vec![
                RobloxAccountConfig {
                    user_id: "valid".into(),
                    cookie_encrypted: "cookie".into(),
                    ..Default::default()
                },
                RobloxAccountConfig {
                    user_id: "   ".into(),
                    cookie_encrypted: "should-skip".into(),
                    ..Default::default()
                },
            ],
        },
        ..Default::default()
    };

    let l = local_config(&config);
    assert_eq!(l.roblox.accounts.len(), 1);
    assert_eq!(l.roblox.accounts[0].user_id, "valid");
}

#[test]
fn telemetry_identifiers_stay_local_and_merge_back() {
    let config = AppConfig {
        telemetry: TelemetryConfig {
            install_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            pending_forget_install_ids: vec!["2d4d97cc-e7e7-4818-8475-5dd327c1eb3d".into()],
            anonymous_id: "797f20fe-94de-4e89-98a2-ae3a3273ad1e".into(),
            ..Default::default()
        },
        ..Default::default()
    };

    let portable = portable_config(&config);
    assert!(portable.telemetry.install_id.is_empty());
    assert!(portable.telemetry.pending_forget_install_ids.is_empty());
    assert!(portable.telemetry.anonymous_id.is_empty());

    let local = local_config(&config);
    assert_eq!(
        local.telemetry.install_id,
        "550e8400-e29b-41d4-a716-446655440000"
    );
    assert_eq!(
        local.telemetry.anonymous_id,
        "797f20fe-94de-4e89-98a2-ae3a3273ad1e"
    );
    assert_eq!(
        local.telemetry.pending_forget_install_ids,
        ["2d4d97cc-e7e7-4818-8475-5dd327c1eb3d"]
    );

    let merged = merge_split_configs(portable, local);
    assert_eq!(merged.telemetry.install_id, config.telemetry.install_id);
    assert_eq!(
        merged.telemetry.pending_forget_install_ids,
        config.telemetry.pending_forget_install_ids
    );
    assert_eq!(merged.telemetry.anonymous_id, config.telemetry.anonymous_id);
}
