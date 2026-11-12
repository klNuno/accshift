//! The settings panel tab bar.

/// The selected settings tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTabBar {
    active_tab: String,
}

impl Default for SettingsTabBar {
    fn default() -> Self {
        Self {
            active_tab: "general".to_string(),
        }
    }
}

impl SettingsTabBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_tab(&self) -> &str {
        &self.active_tab
    }

    pub fn select(&mut self, tab_id: &str) {
        self.active_tab = tab_id.to_string();
    }

    /// A hidden tab falls back to the first visible one (`general` when
    /// none). The new tab when it changed.
    pub fn ensure_active_visible(&mut self, visible_ids: &[String]) -> Option<String> {
        if visible_ids.contains(&self.active_tab) {
            return None;
        }
        self.active_tab = visible_ids
            .first()
            .cloned()
            .unwrap_or_else(|| "general".to_string());
        Some(self.active_tab.clone())
    }
}
