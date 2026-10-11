// Windows startup: the loader maps every statically imported DLL before
// `main`. A bare Rust exe reaches `main` in about 13 ms here; this one took 31.
// The DLLs below are only needed for accessibility clients (UI Automation),
// links and COM, never for the first frame, so they load on first call.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !(windows && msvc) {
        return;
    }
    for dll in [
        "uiautomationcore.dll",
        "shell32.dll",
        "shlwapi.dll",
        "ole32.dll",
        "oleaut32.dll",
        "combase.dll",
    ] {
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
    }
    println!("cargo:rustc-link-arg-bins=delayimp.lib");
}
