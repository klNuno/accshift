//! The bulk edit bar actions on the selected cards.

use super::AppEffect;
use crate::accounts::{Toast, ToastKind};
use crate::stores::ClientStores;
use crate::text::Text;

/// Copies the URLs to the clipboard, one per line; nothing for an empty list.
pub fn copy_bulk_edit_urls(urls: &[String]) -> Vec<AppEffect> {
    if urls.is_empty() {
        return Vec::new();
    }
    vec![AppEffect::CopyBulkUrls(urls.to_vec())]
}

/// The toast after [`AppEffect::CopyBulkUrls`]: `bulkEdit.urlsCopied` with the
/// count, `toast.copyFailed` when the write failed.
pub fn bulk_urls_copied_toast(count: usize, write_ok: bool) -> Toast {
    if write_ok {
        Toast::new(
            Text::with("bulkEdit.urlsCopied", [("count", count as f64)]),
            ToastKind::Success,
        )
    } else {
        Toast::new(Text::key("toast.copyFailed"), ToastKind::Error)
    }
}

/// Writes the colour on every selected id (an empty colour clears it), then
/// bumps the version and shows the toast.
pub fn apply_bulk_edit_card_color(
    stores: &mut ClientStores,
    selected_ids: &[String],
    color: &str,
) -> Vec<AppEffect> {
    if selected_ids.is_empty() {
        return Vec::new();
    }
    stores.set_account_card_colors(selected_ids, color);
    let key = if color.is_empty() {
        "bulkEdit.colorCleared"
    } else {
        "bulkEdit.colorApplied"
    };
    vec![
        AppEffect::CardColorsChanged,
        AppEffect::Toast(Toast::new(
            Text::with(key, [("count", selected_ids.len() as f64)]),
            ToastKind::Success,
        )),
    ]
}
