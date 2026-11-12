pub mod app_dirs;
pub mod config;
pub mod context;
pub mod diagnostics;
pub mod error;
pub mod fs_utils;
pub mod http;
pub mod lock;
pub mod logging;
pub mod os;
pub mod pin;
pub mod platforms;
pub mod runtime;
pub mod secrets;
pub mod snapshot_crypto;
pub mod storage;
pub mod telemetry;
pub mod themes;

pub use context::{AppContext, AppCtx};

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// Files whose comments were moved off the old webview shell. The rest of
    /// the crate is not covered yet, so it is listed here rather than walked.
    const COVERED: &[&str] = &[
        "config",
        "context.rs",
        "diagnostics",
        "lib.rs",
        "lock.rs",
        "logging",
        "logging.rs",
        "pin",
        "pin.rs",
        "snapshot_crypto.rs",
        "storage",
        "storage.rs",
        "telemetry",
    ];

    fn rust_files(path: &Path, out: &mut Vec<PathBuf>) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).expect("list").flatten() {
                rust_files(&entry.path(), out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path.to_path_buf());
        }
    }

    // The GUI is a native egui window. A comment that still points at a
    // webview, a Tauri command or a Svelte file sends the next reader to code
    // that does not exist.
    #[test]
    fn covered_files_do_not_describe_the_retired_shell() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        for entry in COVERED {
            rust_files(&src.join(entry), &mut files);
        }
        assert!(files.len() > COVERED.len(), "{files:?}");

        for file in files {
            let text = std::fs::read_to_string(&file).expect("read");
            // Test modules may name the old shell to guard against it.
            let checked = text.find("mod tests {").map_or(&text[..], |at| &text[..at]);
            let lower = checked.to_ascii_lowercase();
            for needle in ["tauri", "webview", "svelte", "src/lib/"] {
                assert!(
                    !lower.contains(needle),
                    "{} mentions {needle}",
                    file.display()
                );
            }
        }
    }
}
