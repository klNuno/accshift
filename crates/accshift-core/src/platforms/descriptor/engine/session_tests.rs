use super::*;
use crate::platforms::descriptor::load_embedded;

fn epic_checks() -> Vec<IniCheck> {
    let (loaded, errors) = load_embedded();
    assert!(errors.is_empty(), "{errors:?}");
    let epic = loaded.iter().find(|d| d.id == "epic").unwrap();
    let profile = epic.os.values().next().unwrap();
    profile.session.as_ref().unwrap().not_remembered.clone()
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "accshift-descriptor-session-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn with_ini(tag: &str, body: &[u8]) -> PathBuf {
    let dir = scratch(tag);
    fs::write(dir.join("GameUserSettings.ini"), body).unwrap();
    dir
}

#[test]
fn epic_remember_me_disabled_reads_as_not_remembered() {
    let dir = with_ini("disabled", b"[RememberMe]\r\nEnable=False\r\nData=abc\r\n");
    assert!(not_remembered_in(&dir, &epic_checks()));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn epic_remember_me_with_empty_data_reads_as_not_remembered() {
    let dir = with_ini("empty-data", b"[RememberMe]\nEnable=True\nData=\n");
    assert!(not_remembered_in(&dir, &epic_checks()));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn epic_remembered_session_reads_as_remembered() {
    let dir = with_ini(
        "remembered",
        b"\xEF\xBB\xBF[Launcher]\nEnable=False\n[RememberMe]\nEnable=True\nData=QUJD\n",
    );
    assert!(!not_remembered_in(&dir, &epic_checks()));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn missing_or_unreadable_snapshots_mark_nothing() {
    let checks = epic_checks();
    // No file at all.
    let dir = scratch("missing");
    assert!(!not_remembered_in(&dir, &checks));
    // No [RememberMe] section, or the section without the keys.
    let dir = with_ini("no-section", b"[Launcher]\nData=\nEnable=False\n");
    assert!(!not_remembered_in(&dir, &checks));
    let dir = with_ini("no-keys", b"[RememberMe]\n; Enable=False\n");
    assert!(!not_remembered_in(&dir, &checks));
    // Not UTF-8, empty, garbage.
    let dir = with_ini("binary", &[0xff, 0xfe, 0x00, 0x5b, 0x00]);
    assert!(!not_remembered_in(&dir, &checks));
    let dir = with_ini("empty", b"");
    assert!(!not_remembered_in(&dir, &checks));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ini_values_are_read_per_section_case_insensitively() {
    let text = "[a]\nkey = one\n[B]\nKEY=two\nkey=three\n";
    assert_eq!(ini_value(text, "a", "key").as_deref(), Some("one"));
    assert_eq!(ini_value(text, "b", "Key").as_deref(), Some("two"));
    assert_eq!(ini_value(text, "c", "key"), None);
}

#[test]
fn unsafe_account_ids_never_name_a_folder() {
    for id in ["", " ", ".", "..", "../x", "a\\b", "c:x"] {
        assert!(!safe_dir_name(id), "{id}");
    }
    assert!(safe_dir_name("0123456789abcdef0123456789abcdef"));
}

#[test]
fn a_session_check_outside_the_state_files_is_refused() {
    let (loaded, _) = load_embedded();
    let mut epic = loaded.into_iter().find(|d| d.id == "epic").unwrap();
    for profile in epic.os.values_mut() {
        profile.session.as_mut().unwrap().not_remembered[0].snapshot = "../secrets.ini".into();
    }
    let error = epic.validate("epic.json").unwrap_err();
    assert!(format!("{error:?}").contains("notRemembered[0].snapshot"));
}

#[test]
fn a_descriptor_without_a_session_block_still_loads() {
    let (loaded, errors) = load_embedded();
    assert!(errors.is_empty(), "{errors:?}");
    let gog = loaded.iter().find(|d| d.id == "gog").unwrap();
    assert!(gog.os.values().all(|profile| profile.session.is_none()));
}
