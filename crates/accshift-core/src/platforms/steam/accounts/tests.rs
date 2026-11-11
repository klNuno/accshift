use super::{
    clear_integrated_browser_cache_with, copy_game_settings_with, forget_account_with,
    parse_launch_options, remove_loginuser_entry, steam_id_to_account_id,
};
use std::fs;
use std::path::PathBuf;

// SteamID64 whose account ids are 1 and 2, used to build userdata paths.
const FROM_ID: &str = "76561197960265729";
const TO_ID: &str = "76561197960265730";

fn copy_test_root(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("accshift-copygames-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn parse_launch_options_keeps_quoted_groups() {
    let args = parse_launch_options("-silent -applaunch 730 \"-novid -fullscreen\"");
    assert_eq!(
        args,
        vec!["-silent", "-applaunch", "730", "-novid -fullscreen"]
    );
}

#[test]
fn parse_launch_options_handles_single_quotes() {
    let args = parse_launch_options("-foo 'bar baz' -qux");
    assert_eq!(args, vec!["-foo", "bar baz", "-qux"]);
}

#[test]
fn parse_launch_options_handles_escaped_spaces() {
    let args = parse_launch_options("-foo bar\\ baz");
    assert_eq!(args, vec!["-foo", "bar baz"]);
}

// -----------------------------------------------------------------------
// steam_id_to_account_id
// -----------------------------------------------------------------------

#[test]
fn steam_id_to_account_id_known_value() {
    // SteamID64 76561197960265729 -> low 32 bits = 1 (Gabe Newell's account)
    assert_eq!(steam_id_to_account_id("76561197960265729"), Some(1));
}

#[test]
fn steam_id_to_account_id_another_known_value() {
    // 76561197960265728 is the base (0x0110000100000000), low 32 bits = 0
    assert_eq!(steam_id_to_account_id("76561197960265728"), Some(0));
}

#[test]
fn steam_id_to_account_id_large_account_id() {
    // 76561198000000000 -> low 32 bits: 0x0110000100000000 subtracted from base
    // 76561198000000000 = 0x01100001_025317C0, low 32 = 0x025317C0 = 39_734_272
    assert_eq!(
        steam_id_to_account_id("76561198000000000"),
        Some(39_734_272)
    );
}

#[test]
fn steam_id_to_account_id_empty_string() {
    assert_eq!(steam_id_to_account_id(""), None);
}

#[test]
fn steam_id_to_account_id_non_numeric() {
    assert_eq!(steam_id_to_account_id("not_a_number"), None);
}

#[test]
fn steam_id_to_account_id_alphabetic_mixed() {
    assert_eq!(steam_id_to_account_id("7656abc"), None);
}

#[test]
fn steam_id_to_account_id_zero() {
    assert_eq!(steam_id_to_account_id("0"), Some(0));
}

#[test]
fn steam_id_to_account_id_max_u32_low_bits() {
    // 4294967295 = 0xFFFFFFFF, low 32 bits = u32::MAX
    assert_eq!(steam_id_to_account_id("4294967295"), Some(u32::MAX));
}

// -----------------------------------------------------------------------
// remove_loginuser_entry
// -----------------------------------------------------------------------

#[test]
fn remove_loginuser_entry_normal_format_removes_only_target() {
    // Standard Steam layout: every brace sits on its own line.
    let content = "\"users\"\n\
{\n\
\t\"111\"\n\
\t{\n\
\t\t\"AccountName\"\t\"first\"\n\
\t\t\"PersonaName\"\t\"First\"\n\
\t}\n\
\t\"222\"\n\
\t{\n\
\t\t\"AccountName\"\t\"second\"\n\
\t\t\"PersonaName\"\t\"Second\"\n\
\t}\n\
}\n";

    let (out, removed) = remove_loginuser_entry(content, "111");
    assert!(removed);
    assert!(!out.contains("\"111\""));
    assert!(!out.contains("first"));
    // The other account survives intact.
    assert!(out.contains("\"222\""));
    assert!(out.contains("second"));
    assert!(out.contains("Second"));
    // Root structure is preserved.
    assert!(out.starts_with("\"users\"\n{\n"));
    assert!(out.ends_with("}\n"));
}

#[test]
fn remove_loginuser_entry_inline_brace_keeps_following_accounts() {
    // Regression test for the silent-wipe bug (finding K4): a third-party
    // tool condensed the VDF so the opening brace shares the key line.
    // The old brace accounting drifted by one and deleted every account
    // after the target. Here removing "111" must keep "222" and "333".
    let content = "\"users\"\n\
{\n\
\t\"111\" {\n\
\t\t\"AccountName\"\t\"first\"\n\
\t\t\"PersonaName\"\t\"First\"\n\
\t}\n\
\t\"222\" {\n\
\t\t\"AccountName\"\t\"second\"\n\
\t\t\"PersonaName\"\t\"Second\"\n\
\t}\n\
\t\"333\" {\n\
\t\t\"AccountName\"\t\"third\"\n\
\t\t\"PersonaName\"\t\"Third\"\n\
\t}\n\
}\n";

    let (out, removed) = remove_loginuser_entry(content, "111");
    assert!(removed);
    assert!(!out.contains("\"111\""));
    assert!(!out.contains("first"));
    // Both following accounts MUST survive (this is the wipe regression).
    assert!(out.contains("\"222\""));
    assert!(out.contains("second"));
    assert!(out.contains("\"333\""));
    assert!(out.contains("third"));
    assert!(out.ends_with("}\n"));
}

#[test]
fn remove_loginuser_entry_inline_brace_removes_middle_account() {
    // Inline-brace layout, target in the middle: the accounts on both
    // sides must remain.
    let content = "\"users\"\n\
{\n\
\t\"111\" {\n\
\t\t\"AccountName\"\t\"first\"\n\
\t}\n\
\t\"222\" {\n\
\t\t\"AccountName\"\t\"second\"\n\
\t}\n\
\t\"333\" {\n\
\t\t\"AccountName\"\t\"third\"\n\
\t}\n\
}\n";

    let (out, removed) = remove_loginuser_entry(content, "222");
    assert!(removed);
    assert!(out.contains("\"111\""));
    assert!(out.contains("first"));
    assert!(!out.contains("\"222\""));
    assert!(!out.contains("second"));
    assert!(out.contains("\"333\""));
    assert!(out.contains("third"));
}

#[test]
fn remove_loginuser_entry_last_account() {
    // Removing the final account leaves the earlier ones and a valid
    // root block.
    let content = "\"users\"\n\
{\n\
\t\"111\"\n\
\t{\n\
\t\t\"AccountName\"\t\"first\"\n\
\t}\n\
\t\"222\"\n\
\t{\n\
\t\t\"AccountName\"\t\"second\"\n\
\t}\n\
}\n";

    let (out, removed) = remove_loginuser_entry(content, "222");
    assert!(removed);
    assert!(out.contains("\"111\""));
    assert!(out.contains("first"));
    assert!(!out.contains("\"222\""));
    assert!(!out.contains("second"));
    // Root open/close braces are intact.
    assert!(out.starts_with("\"users\"\n{\n"));
    assert!(out.ends_with("}\n"));
}

#[test]
fn remove_loginuser_entry_missing_target_is_noop() {
    let content = "\"users\"\n\
{\n\
\t\"111\"\n\
\t{\n\
\t\t\"AccountName\"\t\"first\"\n\
\t}\n\
}\n";

    let (out, removed) = remove_loginuser_entry(content, "999");
    assert!(!removed);
    assert_eq!(out, content);
}

#[test]
fn remove_loginuser_entry_ignores_braces_inside_strings() {
    // A brace inside a quoted value must not move the depth, otherwise the
    // wrong block boundary is found.
    let content = "\"users\"\n\
{\n\
\t\"111\"\n\
\t{\n\
\t\t\"PersonaName\"\t\"weird { name }\"\n\
\t}\n\
\t\"222\"\n\
\t{\n\
\t\t\"AccountName\"\t\"second\"\n\
\t}\n\
}\n";

    let (out, removed) = remove_loginuser_entry(content, "111");
    assert!(removed);
    assert!(!out.contains("weird { name }"));
    assert!(out.contains("\"222\""));
    assert!(out.contains("second"));
    assert!(out.ends_with("}\n"));
}

// -----------------------------------------------------------------------
// copy_game_settings (stage / backup / swap)
// -----------------------------------------------------------------------

#[test]
fn copy_game_settings_creates_target_when_absent() {
    let root = copy_test_root("absent");
    let src = root.join("userdata").join("1").join("730");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("localconfig.vdf"), b"from-data").unwrap();
    // Destination account dir exists but has no copy of app 730 yet.
    fs::create_dir_all(root.join("userdata").join("2")).unwrap();

    copy_game_settings_with(&root, FROM_ID, TO_ID, "730", || false).unwrap();

    let target = root.join("userdata").join("2").join("730");
    assert_eq!(
        fs::read_to_string(target.join("localconfig.vdf")).unwrap(),
        "from-data"
    );
    // Staging and backup scratch dirs are cleaned up.
    assert!(!root
        .join("userdata")
        .join("2")
        .join(".730.copy-staging")
        .exists());
    assert!(!root
        .join("userdata")
        .join("2")
        .join(".730.copy-backup")
        .exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn copy_game_settings_overwrites_existing_target() {
    let root = copy_test_root("overwrite");
    let src = root.join("userdata").join("1").join("730");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("new.cfg"), b"new").unwrap();
    // Pre-existing target with different content.
    let target = root.join("userdata").join("2").join("730");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("old.cfg"), b"old").unwrap();

    copy_game_settings_with(&root, FROM_ID, TO_ID, "730", || false).unwrap();

    // The new payload replaced the old one; the stale file is gone.
    assert_eq!(fs::read_to_string(target.join("new.cfg")).unwrap(), "new");
    assert!(!target.join("old.cfg").exists());
    // Backup is removed after a successful swap.
    assert!(!root
        .join("userdata")
        .join("2")
        .join(".730.copy-backup")
        .exists());
    let _ = fs::remove_dir_all(&root);
}

// -----------------------------------------------------------------------
// forget_account (stop Steam before reading loginusers.vdf)
// -----------------------------------------------------------------------

const FORGET_BEFORE_EXIT: &str = "\"users\"\n\
{\n\
\t\"111\"\n\
\t{\n\
\t\t\"AccountName\"\t\"first\"\n\
\t\t\"MostRecent\"\t\"1\"\n\
\t}\n\
\t\"222\"\n\
\t{\n\
\t\t\"AccountName\"\t\"second\"\n\
\t\t\"MostRecent\"\t\"0\"\n\
\t}\n\
}\n";

// What Steam writes while it shuts down: the user signed in a third
// account during the session and "222" became the most recent one.
const FORGET_AFTER_EXIT: &str = "\"users\"\n\
{\n\
\t\"111\"\n\
\t{\n\
\t\t\"AccountName\"\t\"first\"\n\
\t\t\"MostRecent\"\t\"0\"\n\
\t}\n\
\t\"222\"\n\
\t{\n\
\t\t\"AccountName\"\t\"second\"\n\
\t\t\"MostRecent\"\t\"1\"\n\
\t}\n\
\t\"333\"\n\
\t{\n\
\t\t\"AccountName\"\t\"third\"\n\
\t\t\"MostRecent\"\t\"0\"\n\
\t}\n\
}\n";

#[test]
fn forget_account_keeps_what_steam_flushed_on_exit() {
    let root = copy_test_root("forget-flush");
    let path = root.join("config").join("loginusers.vdf");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, FORGET_BEFORE_EXIT).unwrap();

    let flush_path = path.clone();
    forget_account_with(&root, "111", move || {
        fs::write(&flush_path, FORGET_AFTER_EXIT).unwrap();
        Ok(super::StopOutcome::Stopped)
    })
    .unwrap();

    let out = fs::read_to_string(&path).unwrap();
    assert!(!out.contains("\"111\""), "{out}");
    assert!(
        out.contains("\"333\""),
        "the flushed account was lost: {out}"
    );
    assert!(
        out.contains("\"second\"\n\t\t\"MostRecent\"\t\"1\""),
        "the flushed MostRecent was reverted: {out}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn forget_account_does_not_stop_steam_for_an_unknown_account() {
    let root = copy_test_root("forget-unknown");
    let path = root.join("config").join("loginusers.vdf");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, FORGET_BEFORE_EXIT).unwrap();

    let mut stopped = false;
    forget_account_with(&root, "999", || {
        stopped = true;
        Ok(super::StopOutcome::Stopped)
    })
    .unwrap();

    assert!(!stopped);
    assert_eq!(fs::read_to_string(&path).unwrap(), FORGET_BEFORE_EXIT);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn forget_account_refuses_an_elevated_steam_without_writing() {
    let root = copy_test_root("forget-elevated");
    let path = root.join("config").join("loginusers.vdf");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, FORGET_BEFORE_EXIT).unwrap();

    let result = forget_account_with(&root, "111", || Ok(super::StopOutcome::NeedsElevation));

    assert!(matches!(result, Err(crate::error::AppError::SteamElevated)));
    assert_eq!(fs::read_to_string(&path).unwrap(), FORGET_BEFORE_EXIT);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn copy_game_settings_rejects_non_numeric_app_id() {
    let root = copy_test_root("badid");
    assert!(copy_game_settings_with(&root, FROM_ID, TO_ID, "../evil", || false).is_err());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn copy_game_settings_refuses_while_steam_runs() {
    let root = copy_test_root("running");
    let src = root.join("userdata").join("1").join("730");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("new.cfg"), b"new").unwrap();
    let target = root.join("userdata").join("2").join("730");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("old.cfg"), b"old").unwrap();

    let result = copy_game_settings_with(&root, FROM_ID, TO_ID, "730", || true);

    assert!(result.is_err());
    assert!(target.join("old.cfg").exists());
    assert!(!target.join("new.cfg").exists());
    let _ = fs::remove_dir_all(&root);
}

// -----------------------------------------------------------------------
// clear_integrated_browser_cache (stop like a switch, relaunch after)
// -----------------------------------------------------------------------

#[test]
fn clearing_the_browser_cache_stops_steam_first_and_relaunches_it() {
    let root = copy_test_root("htmlcache");
    let cache = root.join("htmlcache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("entry"), b"x").unwrap();

    let mut relaunched = false;
    clear_integrated_browser_cache_with(
        &cache,
        || {
            assert!(cache.exists(), "the cache went before Steam was stopped");
            Ok(super::StopOutcome::Stopped)
        },
        || {
            relaunched = true;
            Ok(())
        },
    )
    .unwrap();

    assert!(!cache.exists());
    assert!(relaunched);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn clearing_the_browser_cache_refuses_an_elevated_steam() {
    let root = copy_test_root("htmlcache-elevated");
    let cache = root.join("htmlcache");
    fs::create_dir_all(&cache).unwrap();

    let result = clear_integrated_browser_cache_with(
        &cache,
        || Ok(super::StopOutcome::NeedsElevation),
        || panic!("Steam was never stopped, so it must not be relaunched"),
    );

    assert!(matches!(result, Err(crate::error::AppError::SteamElevated)));
    assert!(cache.exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn library_paths_keep_an_escaped_quote_and_unescape_backslashes() {
    let root = copy_test_root("libraryfolders");
    let steamapps = root.join("steamapps");
    fs::create_dir_all(&steamapps).unwrap();
    let vdf = r#""libraryfolders"
{
	"0"
	{
		"path"		"D:\\Games \"SSD\"\\Steam"
	}
	"1"
	{
		"path"		"E:\\Steam"
	}
}
"#;
    fs::write(steamapps.join("libraryfolders.vdf"), vdf).unwrap();

    let paths = super::load_library_paths(&root);

    // A naive split on `"` cut the first path at its escaped quote.
    assert!(
        paths.contains(&PathBuf::from(r#"D:\Games "SSD"\Steam"#)),
        "{paths:?}"
    );
    assert!(paths.contains(&PathBuf::from(r"E:\Steam")), "{paths:?}");
    assert!(paths.contains(&root));
    assert_eq!(paths.len(), 3, "{paths:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn app_manifest_name_keeps_an_escaped_quote() {
    let manifest = r#""AppState"
{
	"appid"		"730"
	"name"		"The \"Quoted\" Game"
}
"#;
    assert_eq!(
        super::extract_appid_and_name(manifest),
        (Some("730".into()), Some(r#"The "Quoted" Game"#.into()))
    );
}
