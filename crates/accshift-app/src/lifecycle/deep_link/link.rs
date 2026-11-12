//! The switch link: its parser and the account it names.

use crate::js;
use crate::platforms::PlatformAccount;

/// A parsed switch link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepLinkSwitchRequest {
    pub platform_id: String,
    pub account_ref: String,
}

/// The longest link parsed, in UTF-16 units.
pub const MAX_DEEP_LINK_URL_LENGTH: usize = 2048;

/// Parses `accshift://switch/<platformId>/<accountRef>`.
pub fn parse_deep_link(raw_url: &str) -> Option<DeepLinkSwitchRequest> {
    let trimmed = js::trim(raw_url);
    if js::utf16_len(trimmed) > MAX_DEEP_LINK_URL_LENGTH {
        return None;
    }
    const PREFIX: &str = "accshift://";
    let head = trimmed.get(..PREFIX.len())?;
    if !head.eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let rest = &trimmed[PREFIX.len()..];
    let path = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let segments: Vec<String> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| decode_uri_component(segment).unwrap_or_else(|| segment.to_string()))
        .collect();
    if segments.len() == 3 && segments[0].to_lowercase() == "switch" {
        let platform_id = segments[1].to_lowercase();
        let account_ref = js::trim(&segments[2]).to_string();
        if !platform_id.is_empty() && !account_ref.is_empty() {
            return Some(DeepLinkSwitchRequest {
                platform_id,
                account_ref,
            });
        }
    }
    None
}

/// Percent-decodes one URL component: `None` for a `%` without two hex
/// digits, or bytes that are not UTF-8.
pub fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The account a link names: an id match wins; a username or display name
/// must be unique, case-insensitive and trimmed.
pub fn find_account<'a>(
    accounts: &'a [PlatformAccount],
    reference: &str,
) -> Option<&'a PlatformAccount> {
    let needle = js::trim(reference).to_lowercase();
    let key = |value: &str| js::trim(value).to_lowercase();
    if let Some(by_id) = accounts.iter().find(|account| key(&account.id) == needle) {
        return Some(by_id);
    }
    let by_username: Vec<_> = accounts
        .iter()
        .filter(|account| key(&account.username) == needle)
        .collect();
    match by_username.len() {
        1 => return Some(by_username[0]),
        n if n > 1 => return None,
        _ => {}
    }
    let by_display: Vec<_> = accounts
        .iter()
        .filter(|account| key(&account.display_name) == needle)
        .collect();
    (by_display.len() == 1).then(|| by_display[0])
}
