//! Where WebKit keeps the `localStorage` of `tauri://localhost`.
//!
//! WebKit `main` at 1557a275c0cf, `Source/`:
//!
//! - Two layouts, chosen by `UnifiedOriginStorageLevel`
//!   (`WebKit/NetworkProcess/storage/OriginStorageManager.cpp:519-543`):
//!   - `None`: the "custom" path `<localStorageDirectory>/<databaseIdentifier>.localstorage`
//!     (`LocalStorageManager.cpp:41`, `60-67`, `85-95`).
//!   - `Basic` and up: `<generalStorageDirectory>/<top origin>/<origin>/LocalStorage/localstorage.sqlite3`
//!     (`LocalStorageManager.cpp:42`, `97-103`; `OriginStorageManager.cpp:208-215`,
//!     default bucket rooted at the origin folder, `:677`). On first access
//!     WebKit moves an old-layout file there (`:530-536`), so both can exist
//!     on disk across WebKit versions.
//! - Origin folder names: base64url without padding of
//!   SHA-256(UTF-8 origin string + 8-byte salt), salt read from
//!   `<generalStorageDirectory>/salt`
//!   (`NetworkStorageManager.cpp:114-131`, `:243-244`; `WTF/wtf/text/Base64.h:140-142`;
//!   `WTF/wtf/FileSystem.h:152`). For the app's own page the top origin and
//!   the origin are the same.
//! - Origin string `tauri://localhost`, database identifier
//!   `tauri_localhost_0` (no port is port 0):
//!   `WebCore/page/SecurityOriginData.cpp:46-60`, `:117-121`.
//!
//! Linux (WebKitGTK through `webkit2gtk-4.1`, `webkit2gtk-sys-2.0.2/Cargo.toml:58`,
//! not the 2022 GLib API): level `None`
//! (`WebKit/UIProcess/WebsiteData/glib/WebsiteDataStoreGLib.cpp:79-86`), the
//! localStorage directory `<base data>/localstorage`
//! (`WebKit/UIProcess/API/glib/WebKitWebsiteDataManager.cpp:242-243`,
//! `WebKit/UIProcess/WebsiteData/WebsiteDataStore.cpp:2624-2629`). The
//! unified layout under `<base data>/storage`
//! (`WebsiteDataStore.cpp:2549-2552`) is checked too, for a WebKitGTK 6 build.
//!
//! macOS (WKWebView, `defaultDataStore`, app not sandboxed: `entitlements`
//! is `null` in `tauri.conf.json`): level `Standard`
//! (`WebKit/UIProcess/WebsiteData/Cocoa/WebsiteDataStoreCocoa.mm:939-942`),
//! website data under `~/Library/WebKit/<bundle id>/WebsiteData`
//! (`:590-604`, bundle id `:119-129`), general storage `Default`
//! (`:385-390`), old localStorage directory `LocalStorage` (`:470-475`).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The origin the app page has under WebKit.
pub const ORIGIN: &str = "tauri://localhost";
/// `SecurityOriginData::databaseIdentifier` of [`ORIGIN`].
pub const DATABASE_IDENTIFIER: &str = "tauri_localhost_0";

/// The folder name WebKit gives `origin` under a general storage directory
/// whose salt is `salt`.
pub fn origin_folder_name(origin: &str, salt: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(origin.as_bytes());
    hasher.update(salt);
    base64url_no_pad(&hasher.finalize())
}

/// `<general>/<hash>/<hash>/LocalStorage/localstorage.sqlite3` when
/// `<general>/salt` holds the 8-byte salt.
pub fn unified_path(general_storage: &Path) -> Option<PathBuf> {
    let salt = std::fs::read(general_storage.join("salt")).ok()?;
    if salt.len() != 8 {
        return None;
    }
    let folder = origin_folder_name(ORIGIN, &salt);
    Some(
        general_storage
            .join(&folder)
            .join(&folder)
            .join("LocalStorage")
            .join("localstorage.sqlite3"),
    )
}

/// `<dir>/tauri_localhost_0.localstorage`.
pub fn old_path(local_storage_dir: &Path) -> PathBuf {
    local_storage_dir.join(format!("{DATABASE_IDENTIFIER}.localstorage"))
}

/// Linux, newest layout first. `base_data` is the webview data directory:
/// `$XDG_DATA_HOME/<identifier>` or `~/.local/share/<identifier>`.
pub fn linux_candidates(base_data: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    out.extend(unified_path(&base_data.join("storage")));
    out.push(old_path(&base_data.join("localstorage")));
    out
}

/// macOS, newest layout first, under `~/Library/WebKit/<identifier>/WebsiteData`.
pub fn macos_candidates(home: &Path, identifier: &str) -> Vec<PathBuf> {
    let website_data = home
        .join("Library")
        .join("WebKit")
        .join(identifier)
        .join("WebsiteData");
    let mut out = Vec::new();
    out.extend(unified_path(&website_data.join("Default")));
    out.push(old_path(&website_data.join("LocalStorage")));
    out
}

fn base64url_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_matches_rfc4648_vectors_without_padding() {
        assert_eq!(base64url_no_pad(b""), "");
        assert_eq!(base64url_no_pad(b"f"), "Zg");
        assert_eq!(base64url_no_pad(b"fo"), "Zm8");
        assert_eq!(base64url_no_pad(b"foo"), "Zm9v");
        assert_eq!(base64url_no_pad(b"foob"), "Zm9vYg");
        assert_eq!(base64url_no_pad(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn origin_folder_hashes_origin_then_salt() {
        // SHA-256("abc"), FIPS 180-2 vector, in base64url.
        assert_eq!(
            origin_folder_name("ab", b"c"),
            "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0"
        );
    }

    #[test]
    fn origin_folder_is_a_43_char_sha256() {
        let name = origin_folder_name(ORIGIN, &[0; 8]);
        assert_eq!(name.len(), 43);
        assert!(!name.contains(['+', '/', '=']));
    }
}
