//! Finding the launcher's executable on disk.

use super::*;

impl DescriptorService {
    /// Finds the launcher binary: the user's override first, then the places
    /// the descriptor lists, in order.
    ///
    /// The binary itself is outside the sandbox by design. The roots bound
    /// where per-account session data is read and written; the launcher lives
    /// in Program Files and the user may point at it by hand.
    pub(super) fn resolve_executable(&self, app: &dyn AppContext) -> Result<PathBuf, String> {
        self.locate_executable(&config_bridge::path_override(app, &self.descriptor.id))
    }

    pub(super) fn locate_executable(&self, override_path: &str) -> Result<PathBuf, String> {
        let profile = self.profile()?;
        let executable = profile
            .executable
            .as_ref()
            .ok_or_else(|| "Path management not supported".to_string())?;

        if !override_path.is_empty() {
            if let Some(found) = locate_binary(Path::new(override_path), executable) {
                return Ok(found);
            }
        }

        let resolver = self.base_resolver();
        for candidate in &executable.candidates {
            let base = match candidate {
                ExecutableCandidate::Path { template } => match resolver.resolve(template) {
                    Ok(path) => path,
                    // A candidate naming a variable this machine does not have
                    // is not an error, it is a candidate that does not apply.
                    Err(_) => continue,
                },
                ExecutableCandidate::Registry { root, key, value } => {
                    let Some(raw) = reg::read(*root, key, value) else {
                        continue;
                    };
                    PathBuf::from(raw.trim().trim_end_matches(['\\', '/']))
                }
                ExecutableCandidate::UninstallEntry {
                    display_name,
                    value,
                } => {
                    let Some(raw) = reg::uninstall_entry(display_name, value) else {
                        continue;
                    };
                    PathBuf::from(raw.trim().trim_end_matches(['\\', '/']))
                }
            };
            if let Some(found) = locate_binary(&base, executable) {
                return Ok(found);
            }
        }

        Err(format!(
            "Could not locate {} executable",
            self.descriptor.name
        ))
    }

    pub(super) fn launch(&self, app: &dyn AppContext) -> Result<(), String> {
        #[cfg(test)]
        probe(&self.probes.launches);
        let profile = self.profile()?;
        let Some(launch) = profile.launch.as_ref() else {
            return Ok(());
        };
        let executable = self.resolve_executable(app)?;
        let mut command = Command::new(&executable);
        if launch.working_directory_is_install_dir {
            if let Some(install_dir) = executable.parent() {
                command.current_dir(install_dir);
            }
        }
        command.args(launch.args_for(&executable));
        command.spawn().map_err(|e| {
            format!(
                "Could not launch {} {}: {e}",
                self.descriptor.name,
                executable.display()
            )
        })?;
        Ok(())
    }

    pub(super) fn process_names(&self) -> Vec<String> {
        self.profile()
            .map(|profile| profile.close.processes.clone())
            .unwrap_or_default()
    }

    pub(super) fn is_running(&self) -> bool {
        let names = self.process_names();
        if names.is_empty() {
            return false;
        }
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        crate::os::any_process_running(&refs)
    }

    /// Closes the launcher and waits for it to actually exit, so nothing races
    /// its exit-time flush of the session files to disk.
    pub(super) fn quit_and_wait(&self) {
        let Ok(profile) = self.profile() else {
            return;
        };
        if profile.close.processes.is_empty() {
            return;
        }
        let refs: Vec<&str> = profile.close.processes.iter().map(String::as_str).collect();
        crate::os::quit_processes_and_wait(
            &refs,
            profile.close.timeout_ms,
            Duration::from_millis(profile.close.settle_ms),
        );
    }
}
