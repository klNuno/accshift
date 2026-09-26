//! Adding a Roblox account through Quick Login.

#[allow(unused_imports)]
use super::*;

pub fn begin_account_setup(app_handle: &dyn AppContext) -> Result<SetupStatus, String> {
    log_platform_info(
        app_handle,
        "roblox.begin_account_setup",
        "Roblox account setup requested",
        "",
    );

    let response = post_with_csrf(
        "https://apis.roblox.com/auth-token-service/v1/login/create",
        "{}",
    )
    .map_err(|e| format!("Quick Login create failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Quick Login create failed (HTTP {status}): {text}"));
    }

    let login_data = response
        .json::<QuickLoginCreateResponse>()
        .map_err(|e| format!("Could not parse Quick Login response: {e}"))?;

    let setup_id = format!("roblox-setup-{}", Uuid::new_v4());
    let code = login_data.code.clone();

    SETUP_JOBS.insert(
        setup_id.clone(),
        QuickLoginJob {
            code: login_data.code,
            private_key: Zeroizing::new(login_data.private_key),
        },
    )?;

    // accountDisplayName carries the Quick Login code for UI display
    Ok(crate::platforms::make_setup_status(
        &setup_id,
        "waiting_for_login",
        "",
        &code,
        "",
    ))
}

pub fn get_account_setup_status(
    app_handle: &dyn AppContext,
    setup_id: &str,
) -> Result<SetupStatus, String> {
    let job = SETUP_JOBS.touch(setup_id)?;

    // Poll Roblox Quick Login status
    let body = serde_json::json!({
        "code": job.code,
        "privateKey": &*job.private_key,
    });

    let response = post_with_csrf(
        "https://apis.roblox.com/auth-token-service/v1/login/status",
        &body.to_string(),
    )
    .map_err(|e| format!("Quick Login status check failed: {e}"))?;

    let http_status = response.status();
    let response_text = response.text().unwrap_or_default();

    // Do not log the raw body: it comes from an auth service and could carry
    // tokens if the API shape changes. The parsed status is logged below.
    log_platform_info(
        app_handle,
        "roblox.setup_poll",
        "Quick Login status poll",
        format!("httpStatus={http_status}"),
    );

    if !http_status.is_success() {
        return Ok(crate::platforms::make_setup_status(
            setup_id,
            "waiting_for_login",
            "",
            &job.code,
            "",
        ));
    }

    let status_data: QuickLoginStatusResponse = serde_json::from_str(&response_text)
        .map_err(|e| format!("Could not parse Quick Login status: {e}"))?;

    log_platform_info(
        app_handle,
        "roblox.setup_poll",
        "Quick Login status",
        format!("status={}", status_data.status),
    );

    match status_data.status.as_str() {
        "Validated" => {
            log_platform_info(
                app_handle,
                "roblox.setup_poll",
                "Status is Validated, exchanging for cookie",
                "",
            );

            let cookie = match exchange_quick_login_for_cookie(&job.code, &job.private_key) {
                Ok(c) => c,
                Err(e) => {
                    log_platform_info(
                        app_handle,
                        "roblox.setup_poll",
                        "Cookie exchange failed",
                        &e,
                    );
                    return Err(e);
                }
            };

            log_platform_info(
                app_handle,
                "roblox.setup_poll",
                "Cookie obtained, validating",
                "",
            );

            let user = validate_cookie_blocking(&cookie)?;

            log_platform_info(
                app_handle,
                "roblox.setup_poll",
                "Cookie validated, storing account",
                format!(
                    "userId={}; username={}",
                    crate::platforms::redact_id(&user.id.to_string()),
                    crate::platforms::redact_id(&user.name)
                ),
            );

            let encrypted = match crate::os::encrypt_secret(&cookie) {
                Ok(enc) => enc,
                Err(e) => {
                    let msg = format!("Could not encrypt cookie: {e}");
                    log_platform_error(app_handle, "roblox.setup_poll", "Encryption failed", &msg);
                    SETUP_JOBS.remove(setup_id);
                    return Ok(crate::platforms::make_setup_status(
                        setup_id, "failed", "", "", &msg,
                    ));
                }
            };
            if let Err(e) = store_account(app_handle, &user, &encrypted) {
                log_platform_error(
                    app_handle,
                    "roblox.setup_poll",
                    "Account storage failed",
                    &e,
                );
                SETUP_JOBS.remove(setup_id);
                return Ok(crate::platforms::make_setup_status(
                    setup_id, "failed", "", "", &e,
                ));
            }

            SETUP_JOBS.remove(setup_id);

            Ok(crate::platforms::make_setup_status(
                setup_id,
                "ready",
                user.id.to_string(),
                &user.display_name,
                "",
            ))
        }
        "Cancelled" => {
            SETUP_JOBS.remove(setup_id);
            Ok(crate::platforms::make_setup_status(
                setup_id,
                "failed",
                "",
                "",
                "Quick Login was cancelled",
            ))
        }
        // "Created" | "UserLinked" | anything else → still waiting
        _ => Ok(crate::platforms::make_setup_status(
            setup_id,
            "waiting_for_login",
            "",
            &job.code,
            "",
        )),
    }
}

pub fn cancel_account_setup(setup_id: &str) -> Result<(), String> {
    if let Some(job) = SETUP_JOBS.take(setup_id)? {
        let body = serde_json::json!({ "code": job.code });
        let _ = post_with_csrf(
            "https://apis.roblox.com/auth-token-service/v1/login/cancel",
            &body.to_string(),
        );
    }

    Ok(())
}
