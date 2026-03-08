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

        let dst_path = target.join(file_name.as_ref());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, ignored_names)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Could not copy file {}: {e}", src_path.display()))?;
        }
    }

    Ok(())
}

pub fn copy_optional_file(source: &Path, target: &Path) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }

    fs::copy(source, target)
        .map_err(|e| format!("Could not copy file {}: {e}", source.display()))?;

    Ok(())
}
