//! The Linux and macOS installs of `tauri-plugin-updater` 2.11.0
//! (`src/updater.rs` lines 1023-1385). Unlike Windows, nothing exits: the
//! caller relaunches once the files are replaced.
//!
//! Only `std`, `tempfile`, `infer`, `flate2`, `tar`, `dirs` and `osakit`:
//! the file is type-checked for Linux and macOS from Windows by a stub crate
//! that includes it by path (see the M5 result's commands).

use super::error::{UpdaterError, UpdaterResult};
use super::Installer;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// `run_on_main_thread` of the app; the macOS admin prompt runs there.
pub type MainThread<'a> = &'a dyn Fn(Box<dyn FnOnce() + Send>) -> Result<(), String>;

/// updater.rs lines 1039-1045 (Linux): deb and rpm bundles go through the
/// package manager, everything else is an AppImage.
#[cfg(target_os = "linux")]
pub fn install_files(
    installer: Option<Installer>,
    extract_path: &Path,
    bytes: &[u8],
    _main_thread: MainThread<'_>,
) -> UpdaterResult<()> {
    match installer {
        Some(Installer::Deb) => install_deb(extract_path, bytes),
        Some(Installer::Rpm) => install_rpm(extract_path, bytes),
        _ => install_appimage(extract_path, bytes),
    }
}

/// updater.rs lines 1047-1118.
#[cfg(target_os = "linux")]
fn install_appimage(extract_path: &Path, bytes: &[u8]) -> UpdaterResult<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let extract_path_metadata = extract_path.metadata()?;

    for location in tmp_dir_locations(extract_path) {
        let Some(location) = location else { continue };
        let tmp_dir = tempfile::Builder::new()
            .prefix("tauri_current_app")
            .tempdir_in(location)?;
        let tmp_dir_metadata = tmp_dir.path().metadata()?;
        if extract_path_metadata.dev() != tmp_dir_metadata.dev() {
            continue;
        }
        let mut perms = tmp_dir_metadata.permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(tmp_dir.path(), perms)?;

        let tmp_app_image = tmp_dir.path().join("current_app.AppImage");
        let permissions = std::fs::metadata(extract_path)?.permissions();
        std::fs::rename(extract_path, &tmp_app_image)?;

        if infer::archive::is_gz(bytes) {
            let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
            let mut archive = tar::Archive::new(decoder);
            for mut entry in archive.entries()?.flatten() {
                if let Ok(path) = entry.path() {
                    if path.extension() == Some(std::ffi::OsStr::new("AppImage")) {
                        if let Err(err) = entry.unpack(extract_path) {
                            std::fs::rename(&tmp_app_image, extract_path)?;
                            return Err(err.into());
                        }
                        return Ok(());
                    }
                }
            }
            std::fs::rename(&tmp_app_image, extract_path)?;
            return Err(UpdaterError::BinaryNotFoundInArchive);
        }

        return match std::fs::write(extract_path, bytes)
            .and_then(|_| std::fs::set_permissions(extract_path, permissions))
        {
            Err(err) => {
                std::fs::rename(&tmp_app_image, extract_path)?;
                Err(err.into())
            }
            Ok(()) => Ok(()),
        };
    }
    Err(UpdaterError::TempDirNotOnSameMountPoint)
}

/// The three temp locations, in order (lines 1051-1055).
#[cfg(target_os = "linux")]
fn tmp_dir_locations(extract_path: &Path) -> [Option<PathBuf>; 3] {
    [
        Some(std::env::temp_dir()),
        dirs::cache_dir(),
        extract_path.parent().map(Path::to_path_buf),
    ]
}

/// updater.rs lines 1120-1128.
#[cfg(target_os = "linux")]
fn install_deb(extract_path: &Path, bytes: &[u8]) -> UpdaterResult<()> {
    if !infer::archive::is_deb(bytes) {
        return Err(UpdaterError::InvalidUpdaterFormat);
    }
    try_tmp_locations(extract_path, bytes, "dpkg", "-i", "deb")
}

/// updater.rs lines 1130-1136.
#[cfg(target_os = "linux")]
fn install_rpm(extract_path: &Path, bytes: &[u8]) -> UpdaterResult<()> {
    if !infer::archive::is_rpm(bytes) {
        return Err(UpdaterError::InvalidUpdaterFormat);
    }
    try_tmp_locations(extract_path, bytes, "rpm", "-U", "rpm")
}

/// updater.rs lines 1138-1172.
#[cfg(target_os = "linux")]
fn try_tmp_locations(
    extract_path: &Path,
    bytes: &[u8],
    install_cmd: &str,
    install_arg: &str,
    package_extension: &str,
) -> UpdaterResult<()> {
    for location in tmp_dir_locations(extract_path).into_iter().flatten() {
        let prefix = format!("tauri_{package_extension}_update");
        if let Ok(tmp_dir) = tempfile::Builder::new()
            .prefix(&prefix)
            .tempdir_in(location)
        {
            let pkg_path = tmp_dir.path().join(format!("package.{package_extension}"));
            if std::fs::write(&pkg_path, bytes).is_ok() {
                return try_install_with_privileges(&pkg_path, install_cmd, install_arg);
            }
        }
    }
    Err(UpdaterError::TempDirNotFound)
}

/// updater.rs lines 1174-1211: pkexec, then a zenity or kdialog password
/// fed to `sudo -S`, then terminal sudo.
#[cfg(target_os = "linux")]
fn try_install_with_privileges(
    pkg_path: &Path,
    install_cmd: &str,
    install_arg: &str,
) -> UpdaterResult<()> {
    use std::process::Command;
    if let Ok(status) = Command::new("pkexec")
        .arg(install_cmd)
        .arg(install_arg)
        .arg(pkg_path)
        .status()
    {
        if status.success() {
            return Ok(());
        }
    }
    if let Ok(password) = get_password_graphically() {
        if install_with_sudo(pkg_path, &password, install_cmd, install_arg)? {
            return Ok(());
        }
    }
    let status = Command::new("sudo")
        .arg(install_cmd)
        .arg(install_arg)
        .arg(pkg_path)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(UpdaterError::PackageInstallFailed)
    }
}

/// updater.rs lines 1213-1241.
#[cfg(target_os = "linux")]
fn get_password_graphically() -> UpdaterResult<String> {
    use std::process::Command;
    if let Ok(output) = Command::new("zenity")
        .args([
            "--password",
            "--title=Authentication Required",
            "--text=Enter your password to install the update:",
        ])
        .output()
    {
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
    }
    if let Ok(output) = Command::new("kdialog")
        .args(["--password", "Enter your password to install the update:"])
        .output()
    {
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
    }
    Err(UpdaterError::AuthenticationFailed)
}

/// updater.rs lines 1243-1272.
#[cfg(target_os = "linux")]
fn install_with_sudo(
    pkg_path: &Path,
    password: &str,
    install_cmd: &str,
    install_arg: &str,
) -> UpdaterResult<bool> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("sudo")
        .arg("-S")
        .arg(install_cmd)
        .arg(install_arg)
        .arg(pkg_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "{password}")?;
    }
    Ok(child.wait()?.success())
}

/// updater.rs lines 1288-1384 (macOS): unpack the `.app.tar.gz` without
/// its first path component, move the current app aside, move the new one
/// in, asking for admin rights through AppleScript when the move is denied.
#[cfg(target_os = "macos")]
pub fn install_files(
    _installer: Option<Installer>,
    extract_path: &Path,
    bytes: &[u8],
    main_thread: MainThread<'_>,
) -> UpdaterResult<()> {
    use flate2::read::GzDecoder;

    let tmp_backup_dir = tempfile::Builder::new()
        .prefix("tauri_current_app")
        .tempdir()?;
    let tmp_extract_dir = tempfile::Builder::new()
        .prefix("tauri_updated_app")
        .tempdir()?;

    let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(bytes)));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let collected_path: PathBuf = entry.path()?.iter().skip(1).collect();
        let extraction_path = tmp_extract_dir.path().join(&collected_path);
        if let Some(parent) = extraction_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Err(err) = entry.unpack(&extraction_path) {
            std::fs::remove_dir_all(tmp_extract_dir.path()).ok();
            return Err(err.into());
        }
    }

    let move_result = std::fs::rename(extract_path, tmp_backup_dir.path().join("current_app"));
    let need_authorization = match move_result {
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => true,
        Err(err) => {
            std::fs::remove_dir_all(tmp_extract_dir.path()).ok();
            return Err(err.into());
        }
        Ok(()) => false,
    };

    if need_authorization {
        let apple_script = format!(
            "do shell script \"rm -rf '{src}' && mv -f '{new}' '{src}'\" with administrator privileges",
            src = extract_path.display(),
            new = tmp_extract_dir.path().display()
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let res = main_thread(Box::new(move || {
            let mut script =
                osakit::Script::new_from_source(osakit::Language::AppleScript, &apple_script);
            let result = script
                .compile()
                .map_err(|e| e.to_string())
                .and_then(|()| script.execute().map(|_| ()).map_err(|e| e.to_string()));
            let _ = tx.send(result);
        }));
        let result = rx.recv().unwrap_or_else(|_| Err("no answer".into()));
        if res.is_err() || result.is_err() {
            std::fs::remove_dir_all(tmp_extract_dir.path()).ok();
            return Err(UpdaterError::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Failed to move the new app into place",
            )));
        }
    } else {
        if extract_path.exists() {
            std::fs::remove_dir_all(extract_path)?;
        }
        std::fs::rename(tmp_extract_dir.path(), extract_path)?;
    }

    let _ = std::process::Command::new("touch")
        .arg(extract_path)
        .status();
    Ok(())
}
