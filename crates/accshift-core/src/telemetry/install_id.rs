use uuid::Uuid;

/// Generates a UUIDv4 to use as a Mode B install_id.
///
/// Only called when the user opts in to Mode B (explicit consent).
/// Stored in `AppConfig.telemetry.install_id`, local part (not portable).
pub fn generate() -> String {
    Uuid::new_v4().to_string()
}

/// Returns true if the string is a well-formed UUIDv4.
pub fn is_valid(s: &str) -> bool {
    Uuid::parse_str(s)
        .ok()
        .map(|u| u.get_version_num() == 4)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_valid_uuid_v4() {
        let id = generate();
        assert!(is_valid(&id), "generated id is not a valid uuid v4: {id}");
    }

    #[test]
    fn is_valid_rejects_non_uuid() {
        assert!(!is_valid(""));
        assert!(!is_valid("not-a-uuid"));
        assert!(!is_valid("12345678-1234-1234-1234-123456789012"));
    }
}
