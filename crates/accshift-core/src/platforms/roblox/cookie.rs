//! Adding an account from a pasted cookie, and profile lookups.

#[allow(unused_imports)]
use super::*;

/// A pasted cookie Roblox accepted, encrypted and ready to store.
pub struct PastedRobloxAccount {
    pub(super) user: AuthenticatedUserResponse,
    pub(super) encrypted_cookie: String,
}

/// Checks a pasted cookie against Roblox and encrypts it. Network only: the
/// store write is [`store_pasted_account`], which the caller runs under the
/// operation lock so a switch cannot interleave with it.
pub async fn validate_pasted_cookie(
    cookie: String,
    client: reqwest::Client,
) -> Result<PastedRobloxAccount, String> {
    // Accept various paste formats:
    // - raw cookie value
    // - .ROBLOSECURITY:"<cookie>"  or  .ROBLOSECURITY=<cookie>
    // - _|WARNING:...|_<cookie>
    let cookie = {
        let mut raw = cookie.trim().to_string();
        // Strip .ROBLOSECURITY prefix with any separator
        if let Some(rest) = raw.strip_prefix(".ROBLOSECURITY").map(|s| {
            s.trim_start_matches(|c: char| c == ':' || c == '=' || c == '"' || c.is_whitespace())
        }) {
            raw = rest.trim_end_matches('"').to_string();
        }
        // Strip the warning prefix if present
        if let Some(idx) = raw.find("|_") {
            let after = &raw[idx + 2..];
            if !after.is_empty() {
                raw = after.to_string();
            }
        }
        raw.trim().to_string()
    };
    if cookie.is_empty() {
        return Err("Cookie is empty".to_string());
    }

    let resp = client
        .get("https://users.roblox.com/v1/users/authenticated")
        .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
        .send()
        .await
        .map_err(|e| format!("Cookie validation failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Cookie validation failed (HTTP {})", resp.status()));
    }

    let user = resp
        .json::<AuthenticatedUserResponse>()
        .await
        .map_err(|e| format!("Could not parse user response: {e}"))?;

    let encrypted_cookie =
        crate::os::encrypt_secret(&cookie).map_err(|e| format!("Could not encrypt cookie: {e}"))?;
    Ok(PastedRobloxAccount {
        user,
        encrypted_cookie,
    })
}

/// Adds or refreshes the account behind a validated cookie paste.
pub fn store_pasted_account(
    app_handle: &dyn AppContext,
    pasted: PastedRobloxAccount,
) -> Result<RobloxAccount, String> {
    let PastedRobloxAccount {
        user,
        encrypted_cookie,
    } = pasted;
    store_account(app_handle, &user, &encrypted_cookie)?;

    log_platform_info(
        app_handle,
        "roblox.add_by_cookie",
        "Roblox account added via cookie paste",
        format!(
            "userId={}",
            crate::platforms::redact_id(&user.id.to_string())
        ),
    );

    Ok(RobloxAccount {
        user_id: user.id.to_string(),
        username: user.name,
        display_name: user.display_name,
        last_login_at: Some(crate::platforms::now_unix_ms()),
    })
}

pub async fn get_profile_info(
    user_id: String,
    client: reqwest::Client,
) -> Result<RobloxProfileInfo, String> {
    // The id is interpolated into a query string, so digits only.
    if user_id.is_empty() || !user_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid Roblox user id".into());
    }
    let url = format!(
        "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={user_id}&size=150x150&format=Png"
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Thumbnail request failed: {e}"))?;

    if !resp.status().is_success() {
        return Ok(RobloxProfileInfo { avatar_url: None });
    }

    let data = resp
        .json::<ThumbnailResponse>()
        .await
        .map_err(|e| format!("Could not parse thumbnail response: {e}"))?;

    Ok(RobloxProfileInfo {
        avatar_url: data.data.first().and_then(|e| e.image_url.clone()),
    })
}
