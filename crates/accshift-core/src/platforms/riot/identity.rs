//! Account identity: aliases, labels and the live identity cross-check.

use super::*;

pub(super) fn trim_or_empty(value: &str) -> String {
    value.trim().to_string()
}

pub(super) fn format_account_alias(name: &str, tag_line: &str) -> String {
    let name = name.trim();
    let tag_line = tag_line.trim();
    match (name.is_empty(), tag_line.is_empty()) {
        (true, true) => String::new(),
        (false, true) => name.to_string(),
        _ => format!("{name}#{tag_line}"),
    }
}

pub(super) fn current_account_alias(profile: &RiotProfileConfig) -> String {
    format_account_alias(&profile.account_name, &profile.account_tag_line)
}

pub(super) fn is_generated_profile_label(label: &str) -> bool {
    let Some(index) = label.strip_prefix("Riot Profile ") else {
        return false;
    };
    !index.is_empty() && index.chars().all(|ch| ch.is_ascii_digit())
}

/// Returns whether anything actually changed, so callers polling once a second
/// can skip the config write when the detected identity already matches.
///
/// A profile that already belongs to an account (it has a puuid) is only
/// updated from an identity carrying that same puuid: another account signed
/// in, or an alias read without its puuid, must not rename it.
pub(super) fn apply_detected_identity(
    profile: &mut RiotProfileConfig,
    identity: &RiotDetectedIdentity,
) -> bool {
    let stored_puuid = profile.account_puuid.trim();
    if !stored_puuid.is_empty() && !stored_puuid.eq_ignore_ascii_case(identity.account_puuid.trim())
    {
        return false;
    }
    let previous_alias = current_account_alias(profile);
    let next_alias = format_account_alias(&identity.account_name, &identity.account_tag_line);
    let should_sync_label = profile.label.trim().is_empty()
        || is_generated_profile_label(profile.label.trim())
        || (!previous_alias.is_empty()
            && profile.label.trim().eq_ignore_ascii_case(&previous_alias));

    let previous_account_name = profile.account_name.clone();
    let previous_account_tag_line = profile.account_tag_line.clone();
    let previous_account_puuid = profile.account_puuid.clone();
    let previous_label = profile.label.clone();

    profile.account_name = trim_or_empty(&identity.account_name);
    profile.account_tag_line = trim_or_empty(&identity.account_tag_line);
    profile.account_puuid = trim_or_empty(&identity.account_puuid);

    if should_sync_label && !next_alias.is_empty() {
        profile.label = next_alias;
    }

    profile.account_name != previous_account_name
        || profile.account_tag_line != previous_account_tag_line
        || profile.account_puuid != previous_account_puuid
        || profile.label != previous_label
}

/// How the signed-in Riot account relates to a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IdentityCheck {
    /// The live puuid is the profile's.
    Match,
    /// The live puuid belongs to another account.
    Mismatch,
    /// The profile has no puuid yet, so the live account can be adopted.
    Unclaimed,
    /// The live puuid could not be read (client closed, local API down).
    Unknown,
}

pub(super) fn check_live_identity(
    profile: &RiotProfileConfig,
    live: Option<&RiotDetectedIdentity>,
) -> IdentityCheck {
    let Some(live_puuid) = live
        .map(|identity| identity.account_puuid.trim())
        .filter(|puuid| !puuid.is_empty())
    else {
        return IdentityCheck::Unknown;
    };
    let stored = profile.account_puuid.trim();
    if stored.is_empty() {
        IdentityCheck::Unclaimed
    } else if stored.eq_ignore_ascii_case(live_puuid) {
        IdentityCheck::Match
    } else {
        IdentityCheck::Mismatch
    }
}

/// Profile states whose live session a switch backs up before leaving them.
pub(super) const SWITCH_BACKUP_STATES: &[&str] = &["ready", "awaiting_capture", "setup_pending"];
/// Profile states whose live session a new setup backs up before clearing it.
/// A setup restarted over its own pending profile has nothing worth keeping.
pub(super) const SETUP_BACKUP_STATES: &[&str] = &["ready", "awaiting_capture"];

/// What to do with the live session before it is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutgoingBackup {
    /// Nothing worth saving, or no profile to save it into.
    Skip,
    /// Save it into the profile. `adopt_identity` also records the live
    /// account's name and puuid on the profile.
    Backup { adopt_identity: bool },
    /// The live session is another account's: saving it would overwrite the
    /// profile's snapshot with the wrong account.
    IdentityMismatch,
}

pub(super) fn plan_outgoing_backup(
    profile: Option<&RiotProfileConfig>,
    eligible_states: &[&str],
    has_live_tokens: bool,
    live: Option<&RiotDetectedIdentity>,
) -> OutgoingBackup {
    let Some(profile) = profile else {
        return OutgoingBackup::Skip;
    };
    if !eligible_states.contains(&profile.snapshot_state.as_str()) || !has_live_tokens {
        return OutgoingBackup::Skip;
    }
    match check_live_identity(profile, live) {
        IdentityCheck::Mismatch => OutgoingBackup::IdentityMismatch,
        IdentityCheck::Match | IdentityCheck::Unclaimed => OutgoingBackup::Backup {
            adopt_identity: true,
        },
        // The client is closed or its local API is down. The flushed file is
        // most likely the profile's own session with rotated tokens, and
        // skipping it would leave the profile on tokens the server already
        // revoked. Keep the backup, but trust nothing about the identity.
        IdentityCheck::Unknown => OutgoingBackup::Backup {
            adopt_identity: false,
        },
    }
}

/// `apply_detected_identity` for a profile still in setup: whoever signs in to
/// the client the setup launched is the account being added, even when an
/// earlier poll saw another one (the user switched accounts before finishing).
pub(super) fn adopt_detected_identity(
    profile: &mut RiotProfileConfig,
    identity: &RiotDetectedIdentity,
) -> bool {
    if check_live_identity(profile, Some(identity)) != IdentityCheck::Mismatch {
        return apply_detected_identity(profile, identity);
    }
    profile.account_puuid.clear();
    apply_detected_identity(profile, identity);
    true
}

pub(super) fn make_setup_status(
    profile: &RiotProfileConfig,
    state: &str,
    error_message: impl Into<String>,
) -> RiotProfileSetupStatus {
    RiotProfileSetupStatus {
        profile_id: profile.id.clone(),
        state: state.to_string(),
        account_id: profile.id.clone(),
        account_display_name: current_account_alias(profile),
        error_message: error_message.into(),
    }
}
