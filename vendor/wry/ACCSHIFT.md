# wry 0.55.1, patched

Upstream wry 0.55.1 from crates.io (Apache-2.0 OR MIT, licenses alongside), wired in through `[patch.crates-io]` in the root `Cargo.toml`. Examples, changelog and upstream docs are left out. Only the Windows backend changes, and every change carries an `Accshift patch` comment:

- `src/webview2/prestart.rs`, new: starts WebView2 on a hidden message-only window before the app builds its own. The Windows launcher (`src-tauri/src/launcher.rs`) calls it first thing, then loads the app DLL, whose first webview takes the controller over if its environment options match.
- `src/webview2/mod.rs`: `new_in_hwnd` takes the prestarted controller; the browser arguments, environment options and controller request are split into helpers that the prestart reuses; `add_script_to_execute_on_document_created` no longer waits for each completion.
- `src/lib.rs`: re-exports the prestart API.

## Updating

Tauri pins wry through `tauri-runtime-wry`. When it moves to another wry version, this patch stops applying (cargo warns `patch ... was not used`) and the launcher fails to build, since upstream has no `webview2_prestart`. To move along: copy the new upstream crate over this directory, drop `examples/`, the `[[example]]` and dev-dependency sections of `Cargo.toml`, and replay the three changes above.
