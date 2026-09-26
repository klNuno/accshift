use super::*;
use crate::diagnostics::test_support::TestCtx;
use serde_json::Value;

fn read_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("every line must be JSON"))
        .collect()
}

// The panic hook runs on whatever thread panicked, which may be the thread
// already inside `with_sink`. Holding the mutex here reproduces that
// without a real panic: the hook path must report "not written" instead of
// waiting on a mutex it would never get back.
#[test]
fn the_panic_hook_path_skips_a_held_sink() {
    let ctx = TestCtx::ctx("logging-panic-hook-try-lock");
    let path = log_file_path(&*ctx).expect("path");

    let held = sinks().lock().unwrap_or_else(|error| error.into_inner());
    let wrote = try_append_app_log(&*ctx, "error", "rust.panic", "held", None);
    drop(held);

    assert_eq!(wrote, Ok(false), "a held sink must not block the hook");
    assert!(
        read_lines(&path).is_empty(),
        "the skipped record wrote nothing"
    );

    // The same call with nothing held takes the record. Retried because
    // every other logging test in this binary locks the same map, so one
    // attempt can lose the race with a test running beside this one.
    let mut wrote = Ok(false);
    for _ in 0..1_000 {
        wrote = try_append_app_log(&*ctx, "error", "rust.panic", "free", None);
        if wrote == Ok(true) {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(wrote, Ok(true), "an unheld sink still takes the record");

    let messages: Vec<String> = read_lines(&path)
        .iter()
        .map(|record| record["message"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(messages, vec!["free".to_string()]);
}

// The 38 existing call sites still write this shape, and external readers
// (support, the user's own grep) already know it.
#[test]
fn the_legacy_line_shape_is_unchanged() {
    let ctx = TestCtx::ctx("logging-legacy-shape");
    append_app_log(&*ctx, "warning", "steam.switch", "hello", Some("detail")).expect("append");

    let path = log_file_path(&*ctx).expect("path");
    let records = read_lines(&path);
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record["level"], serde_json::json!("warning"));
    assert_eq!(record["source"], serde_json::json!("steam.switch"));
    assert_eq!(record["message"], serde_json::json!("hello"));
    assert_eq!(record["details"], serde_json::json!("detail"));
    assert!(record["tsMs"].as_u64().is_some());
    assert_eq!(
        record.as_object().expect("object").len(),
        5,
        "the facade must not grow columns"
    );
}

#[test]
fn lines_another_process_appended_count_toward_the_cap() {
    let ctx = TestCtx::ctx("logging-shared-writer");
    let path = log_file_path(&*ctx).expect("path");
    append_app_log(&*ctx, "info", "test", "first", None).expect("append");

    // Another process (the CLI) fills the same file behind this sink's back.
    {
        let mut other = OpenOptions::new().append(true).open(&path).unwrap();
        other
            .write_all(&vec![b'y'; MAX_LOG_FILE_BYTES as usize])
            .unwrap();
    }
    append_app_log(&*ctx, "info", "test", "after", None).expect("append");

    assert!(
        rotated_path(&path, 1).exists(),
        "the shared file crossed the cap and must have rotated"
    );
    assert!(fs::metadata(&path).unwrap().len() <= MAX_LOG_FILE_BYTES);
}

#[test]
fn the_active_file_stays_under_the_size_cap() {
    let ctx = TestCtx::ctx("logging-rotation");
    let path = log_file_path(&*ctx).expect("path");
    // The facade caps a message at 512 bytes, so the line size is known:
    // roughly 620 bytes each, and 12000 of them cross the 2 MiB cap three
    // times over.
    let filler = "x".repeat(4_000);
    for _ in 0..12_000 {
        append_app_log(&*ctx, "info", "test", &filler, None).expect("append");
    }

    let active = fs::metadata(&path).expect("metadata").len();
    assert!(
        active <= MAX_LOG_FILE_BYTES,
        "active file is {active} bytes, cap is {MAX_LOG_FILE_BYTES}"
    );
    assert!(
        rotated_path(&path, 1).exists(),
        "crossing the cap must produce a rotated file"
    );

    // The announced budget is a ceiling for the whole chain, not per file.
    let mut total = active;
    for index in 1..=ROTATED_FILES_KEPT {
        total += fs::metadata(rotated_path(&path, index))
            .map(|meta| meta.len())
            .unwrap_or(0);
    }
    assert!(
        total <= disk_budget_bytes(),
        "chain is {total} bytes, budget is {}",
        disk_budget_bytes()
    );
    assert!(
        !rotated_path(&path, ROTATED_FILES_KEPT + 1).exists(),
        "nothing may survive past the last kept slot"
    );
    assert!(
        !rotating_path(&path).exists(),
        "the staging name must not survive a finished rotation"
    );
}

#[test]
fn rotation_announces_itself_in_the_new_file() {
    let ctx = TestCtx::ctx("logging-rotation-notice");
    let path = log_file_path(&*ctx).expect("path");
    let filler = "x".repeat(4_000);
    for _ in 0..4_000 {
        append_app_log(&*ctx, "info", "test", &filler, None).expect("append");
    }

    let first = read_lines(&path).into_iter().next().expect("a first line");
    assert_eq!(first["code"], serde_json::json!("log.rotated"));
    assert_eq!(first["fields"]["reason"], serde_json::json!("size"));
    assert!(first["fields"]["bytes"].as_u64().is_some_and(|b| b > 0));
}

#[test]
fn a_session_rotates_the_previous_one_out_of_the_way() {
    let ctx = TestCtx::ctx("logging-session");
    append_app_log(&*ctx, "info", "test", "from the previous session", None).expect("append");

    begin_log_session(&*ctx).expect("session");

    let path = log_file_path(&*ctx).expect("path");
    let previous = read_lines(&rotated_path(&path, 1));
    assert_eq!(previous.len(), 1);
    assert_eq!(
        previous[0]["message"],
        serde_json::json!("from the previous session")
    );

    let current = read_lines(&path);
    assert_eq!(current[0]["code"], serde_json::json!("log.rotated"));
    assert!(
        current
            .iter()
            .any(|record| record["code"] == serde_json::json!("app.session.started")),
        "a session must be able to say when it started"
    );
}

// Staging rename fails (a non-empty directory occupies the temp name, the
// same outcome as another process holding app.log without FILE_SHARE_DELETE
// on Windows). The oldest slot must still be there, and writes must work.
#[test]
fn a_failed_rotation_does_not_delete_the_chain() {
    let ctx = TestCtx::ctx("logging-rotate-fail");
    let path = log_file_path(&*ctx).expect("path");
    ensure_parent(&path).expect("parent");

    append_app_log(&*ctx, "info", "test", "active", None).expect("seed live");
    for index in 1..=ROTATED_FILES_KEPT {
        fs::write(
            rotated_path(&path, index),
            format!("{{\"tsMs\":{index},\"message\":\"slot{index}\"}}\n"),
        )
        .expect("seed slot");
    }

    let staging = rotating_path(&path);
    fs::create_dir_all(&staging).expect("staging dir");
    fs::write(staging.join("blocker"), b"x").expect("block rename");

    begin_log_session(&*ctx).expect("session must keep writing");

    let oldest = rotated_path(&path, ROTATED_FILES_KEPT);
    assert!(
        oldest.exists(),
        "a failed live rename must not drop the oldest file"
    );
    assert_eq!(
        fs::read_to_string(&oldest).expect("read oldest"),
        format!("{{\"tsMs\":{ROTATED_FILES_KEPT},\"message\":\"slot{ROTATED_FILES_KEPT}\"}}\n")
    );
    let newest_rotated = fs::read_to_string(rotated_path(&path, 1)).expect("read .1");
    assert!(
        newest_rotated.contains("slot1"),
        "the numbered chain must stay where it was: {newest_rotated}"
    );

    append_app_log(&*ctx, "info", "test", "after-failed-rotate", None)
        .expect("app.log must still accept writes");
    let current = fs::read_to_string(&path).expect("read live");
    assert!(
        current.contains("after-failed-rotate"),
        "writes after a failed rotate land in the live file: {current}"
    );

    let _ = fs::remove_dir_all(&staging);
}

// An empty file is not worth a rotation slot: the CLI runs often and would
// otherwise push the GUI's history out of the chain in four invocations.
#[test]
fn an_empty_session_does_not_burn_a_slot() {
    let ctx = TestCtx::ctx("logging-session-empty");
    begin_log_session(&*ctx).expect("session");

    let path = log_file_path(&*ctx).expect("path");
    assert!(!rotated_path(&path, 1).exists());
}

#[test]
fn a_legacy_previous_file_joins_the_chain() {
    let ctx = TestCtx::ctx("logging-legacy-migration");
    let path = log_file_path(&*ctx).expect("path");
    ensure_parent(&path).expect("parent");
    fs::write(
        path.with_file_name(LEGACY_PREVIOUS_LOG_FILE_NAME),
        "{\"tsMs\":1,\"level\":\"info\",\"source\":\"old\",\"message\":\"kept\"}\n",
    )
    .expect("seed");

    begin_log_session(&*ctx).expect("session");

    assert!(!path.with_file_name(LEGACY_PREVIOUS_LOG_FILE_NAME).exists());
    let migrated = read_lines(&rotated_path(&path, 1));
    assert_eq!(migrated[0]["message"], serde_json::json!("kept"));
}

// The other process rotated. Our open handle now points at app.1.log, and
// appending into it would write the newest records into the oldest file.
#[test]
fn a_rotation_by_another_process_forces_a_reopen() {
    let ctx = TestCtx::ctx("logging-generation");
    append_app_log(&*ctx, "info", "test", "before", None).expect("append");
    let path = log_file_path(&*ctx).expect("path");

    // Simulate the peer: rename the file and bump the counter, exactly what
    // `rotate` does, without going through this process' sink.
    {
        let mut map = sinks().lock().expect("sinks");
        let sink = map.get_mut(&path).expect("sink");
        let stale_generation = sink.generation;
        sink.file = None;
        fs::rename(&path, rotated_path(&path, 1)).expect("rename");

        let lock_path = path.with_file_name(LOG_LOCK_FILE_NAME);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .expect("lock file");
        let mut handle = &lock;
        handle
            .write_all(&(stale_generation + 1).to_le_bytes())
            .expect("bump");
    }

    append_app_log(&*ctx, "info", "test", "after", None).expect("append");

    let current = read_lines(&path);
    assert_eq!(current.len(), 1, "the new file holds only the new record");
    assert_eq!(current[0]["message"], serde_json::json!("after"));
    let rotated = read_lines(&rotated_path(&path, 1));
    assert_eq!(rotated[0]["message"], serde_json::json!("before"));
}

#[test]
fn files_older_than_the_retention_window_are_purged() {
    let ctx = TestCtx::ctx("logging-retention");
    let path = log_file_path(&*ctx).expect("path");
    ensure_parent(&path).expect("parent");

    let stale = rotated_path(&path, 2);
    fs::write(&stale, "{\"tsMs\":1}\n").expect("seed");
    let ancient =
        SystemTime::now() - std::time::Duration::from_secs((RETENTION_DAYS + 1) * 24 * 60 * 60);
    let handle = OpenOptions::new().write(true).open(&stale).expect("open");
    handle
        .set_modified(ancient)
        .expect("backdate the file so the sweep can see it as old");
    drop(handle);

    let purged = purge(&path);

    assert_eq!(purged.files, 1);
    assert!(!stale.exists());
}

#[test]
fn the_announced_budget_matches_the_policy() {
    let policy = retention_policy();
    assert_eq!(
        policy["diskBudgetBytes"].as_u64(),
        Some(disk_budget_bytes())
    );
    assert_eq!(
        disk_budget_bytes(),
        MAX_LOG_FILE_BYTES * (u64::from(ROTATED_FILES_KEPT) + 1)
    );
}
