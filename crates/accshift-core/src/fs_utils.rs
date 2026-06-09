use std::fs;
use std::path::Path;

fn should_ignore_name(name: &str, ignored_names: &[&str]) -> bool {
    ignored_names
        .iter()
        .any(|ignored| ignored.eq_ignore_ascii_case(name))
}

pub fn copy_dir_recursive(
    source: &Path,
    target: &Path,
    ignored_names: &[&str],
) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }

    fs::create_dir_all(target)
        .map_err(|e| format!("Could not create directory {}: {e}", target.display()))?;

    for entry in fs::read_dir(source)
        .map_err(|e| format!("Could not read directory {}: {e}", source.display()))?
    {
        let entry = entry.map_err(|e| format!("Could not read directory entry: {e}"))?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if should_ignore_name(&file_name, ignored_names) {
            continue;
        }

        // Skip symlinks entirely: following them can escape the source tree
        // or loop forever (symlink to an ancestor).
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Could not stat {}: {e}", src_path.display()))?;
        if file_type.is_symlink() {
            continue;
        }

        let dst_path = target.join(file_name.as_ref());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, ignored_names)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Could not copy file {}: {e}", src_path.display()))?;
        }
    }

    Ok(())
}
