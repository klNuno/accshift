//! The migration against a config write landing at the same time.

use super::super::store::config_io_test_mutex;
use super::*;
use std::sync::{Arc, Barrier};

struct Dir(std::path::PathBuf);

impl AppContext for Dir {
    fn app_config_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_local_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_cache_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
}

/// The telemetry id written while the boot migrates the pre-1.0 file
/// survives the migration, whichever of the two writes first.
#[test]
fn a_write_racing_the_migration_is_kept() {
    let _guard = config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = std::env::temp_dir().join(format!("accshift-migrate-race-{}", std::process::id()));
    for round in 0..60u64 {
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let ctx = Arc::new(Dir(root.clone()));
        let legacy = crate::storage::legacy_config_path(&*ctx).unwrap();
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(
            &legacy,
            br#"{"riot":{"profiles":[{"id":"r1","label":"Main"}]},"window_width":1180}"#,
        )
        .unwrap();
        let start = Arc::new(Barrier::new(2));
        let writer = {
            let (ctx, start) = (Arc::clone(&ctx), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                // Spread the write over the migration's read, merge and save.
                std::thread::sleep(std::time::Duration::from_micros(round * 25));
                update_config(&*ctx, |cfg| cfg.telemetry.anonymous_id = "racing-id".into())
                    .unwrap();
            })
        };
        start.wait();
        assert!(matches!(migrate_legacy_config(&*ctx), Some(Ok(())) | None));
        writer.join().unwrap();
        // Whichever ran second, the pair on disk holds both.
        *super::super::store::config_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        let cfg = load_config(&*ctx);
        assert_eq!(cfg.telemetry.anonymous_id, "racing-id", "round {round}");
        assert_eq!(cfg.riot.profiles.len(), 1, "round {round}");
        assert_eq!(cfg.window_width, Some(1180.0), "round {round}");
    }
    let _ = std::fs::remove_dir_all(&root);
}
