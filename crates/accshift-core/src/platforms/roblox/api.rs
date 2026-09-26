//! Roblox web API calls (blocking): CSRF, cookie validation, Quick Login and session probes.

#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(super) struct QuickLoginCreateResponse {
    pub(super) code: String,
    #[serde(rename = "privateKey")]
    pub(super) private_key: String,
}

#[derive(Deserialize)]
pub(super) struct QuickLoginStatusResponse {
    pub(super) status: String,
}

#[derive(Deserialize)]
pub(super) struct AuthenticatedUserResponse {
    pub(super) id: u64,
    pub(super) name: String,
    #[serde(rename = "displayName")]
    pub(super) display_name: String,
}

#[derive(Deserialize)]
pub(super) struct ThumbnailEntry {
    #[serde(rename = "imageUrl")]
    pub(super) image_url: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ThumbnailResponse {
    pub(super) data: Vec<ThumbnailEntry>,
}

pub(super) fn blocking_client() -> Result<&'static reqwest::blocking::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    let result = CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {e}"))
    });
    result.as_ref().map_err(|e| e.clone())
}

pub(super) fn csrf_cache() -> &'static Mutex<String> {
    static CSRF: OnceLock<Mutex<String>> = OnceLock::new();
    CSRF.get_or_init(|| Mutex::new(String::new()))
}

/// POST with automatic CSRF retry. Roblox returns 403 + x-csrf-token on first attempt.
/// `extra_headers` lets callers inject additional headers (e.g. Referer) without duplicating
/// the retry logic.
pub(super) fn post_with_csrf_and_headers(
    url: &str,
    body: &str,
    extra_headers: &[(&str, &str)],
) -> Result<reqwest::blocking::Response, String> {
    let cached_csrf = csrf_cache().lock().map(|g| g.clone()).unwrap_or_default();

    let mut request = blocking_client()?
        .post(url)
        .header("Content-Type", "application/json")
        .header("X-CSRF-TOKEN", &cached_csrf);
    for &(k, v) in extra_headers {
        request = request.header(k, v);
    }
    let response = request
        .body(body.to_string())
        .send()
        .map_err(|e| format!("Request failed: {e}"))?;

    if response.status().as_u16() == 403 {
        let new_csrf = response
            .headers()
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !new_csrf.is_empty() {
            if let Ok(mut cache) = csrf_cache().lock() {
                *cache = new_csrf.clone();
            }

            let mut retry = blocking_client()?
                .post(url)
                .header("Content-Type", "application/json")
                .header("X-CSRF-TOKEN", &new_csrf);
            for &(k, v) in extra_headers {
                retry = retry.header(k, v);
            }
            return retry
                .body(body.to_string())
                .send()
                .map_err(|e| format!("Request retry failed: {e}"));
        }
    }

    Ok(response)
}

pub(super) fn post_with_csrf(url: &str, body: &str) -> Result<reqwest::blocking::Response, String> {
    post_with_csrf_and_headers(url, body, &[])
}

pub(super) fn validate_cookie_blocking(cookie: &str) -> Result<AuthenticatedUserResponse, String> {
    let response = blocking_client()?
        .get("https://users.roblox.com/v1/users/authenticated")
        .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
        .send()
        .map_err(|e| format!("Cookie validation request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Cookie validation failed (HTTP {})",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .map_err(|e| format!("Could not read user response: {e}"))?;
    if bytes.len() as u64 > ROBLOX_AUTH_RESPONSE_MAX_BYTES {
        return Err("Roblox user response is too large".into());
    }

    serde_json::from_slice::<AuthenticatedUserResponse>(&bytes)
        .map_err(|e| format!("Could not parse user response: {e}"))
}

/// Ask Roblox whether a cookie is still a valid session. `Some(true)` alive,
/// `Some(false)` definitively rejected (401/403), `None` when we couldn't tell
/// (network error, rate limit, 5xx) so the caller never flags a false positive.
pub(super) fn probe_cookie_alive(cookie: &str) -> Option<bool> {
    let response = blocking_client()
        .ok()?
        .get("https://users.roblox.com/v1/users/authenticated")
        .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
        .send()
        .ok()?;
    let status = response.status().as_u16();
    match status {
        200 => Some(true),
        401 | 403 => Some(false),
        _ => None,
    }
}

/// Cap on concurrent `probe_cookie_alive` calls: enough to hide the network
/// latency of large account lists without hammering the auth endpoint into
/// rate-limiting us.
pub(super) const MAX_CONCURRENT_PROBES: usize = 8;

/// User ids whose stored session cookie Roblox reports as invalid, so the UI can
/// badge them before the user clicks into a switch that would fail. Empty cookies
/// count as dead; unknown results (network/rate-limit) are omitted.
///
/// Probes run in parallel (bounded by `MAX_CONCURRENT_PROBES`): each one is a
/// full HTTP round-trip with a 10s timeout, so probing N accounts sequentially
/// made the badge refresh crawl.
pub fn dead_session_user_ids(app_handle: &dyn AppContext) -> Vec<String> {
    let mut dead: Vec<String> = Vec::new();
    let mut to_probe: Vec<(String, Zeroizing<String>)> = Vec::new();

    for account in load_account_configs(app_handle) {
        let Ok(cookie) = crate::os::decrypt_secret(&account.cookie_encrypted) else {
            continue;
        };
        if cookie.trim().is_empty() {
            dead.push(account.user_id);
        } else {
            to_probe.push((account.user_id, Zeroizing::new(cookie)));
        }
    }

    for chunk in to_probe.chunks(MAX_CONCURRENT_PROBES) {
        let alive_flags: Vec<Option<bool>> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|(_, cookie)| scope.spawn(move || probe_cookie_alive(cookie)))
                .collect();
            handles
                .into_iter()
                // A panicked probe reads as None: unknown, never flagged dead.
                .map(|handle| handle.join().ok().flatten())
                .collect()
        });
        for ((user_id, _), alive) in chunk.iter().zip(alive_flags) {
            if alive == Some(false) {
                dead.push(user_id.clone());
            }
        }
    }

    dead
}

pub(super) fn extract_roblosecurity_cookie(headers: &reqwest::header::HeaderMap) -> Option<String> {
    for value in headers.get_all("set-cookie") {
        let Ok(s) = value.to_str() else { continue };
        if let Some(rest) = s.strip_prefix(".ROBLOSECURITY=") {
            let cookie = rest.split(';').next().unwrap_or(rest).trim();
            if !cookie.is_empty() {
                return Some(cookie.to_string());
            }
        }
    }
    None
}

pub(super) fn exchange_quick_login_for_cookie(
    code: &str,
    private_key: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "ctype": "AuthToken",
        "cvalue": code,
        "password": private_key,
    });

    let response = post_with_csrf_and_headers(
        "https://auth.roblox.com/v2/login",
        &body.to_string(),
        &[("Referer", "https://www.roblox.com")],
    )?;

    if !response.status().is_success() {
        // Do not include the raw response body in the error: this endpoint
        // receives the Quick Login code and session private key, and its error
        // body could echo secret-shaped content that would then reach app.log.
        // The sibling /login/status path guards the same way.
        return Err(format!(
            "Login exchange failed (HTTP {})",
            response.status()
        ));
    }

    extract_roblosecurity_cookie(response.headers())
        .ok_or_else(|| "No .ROBLOSECURITY cookie in login response".to_string())
}
