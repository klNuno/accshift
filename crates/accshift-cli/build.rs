// Windows: the version resource the GUI also carries, with the same product
// name and version. Code signing restricts signed files to these values. The
// VC runtime is linked statically like the GUI's, so the standalone CLI runs
// on a machine without the VC++ redistributable.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !(windows && msvc) {
        return;
    }
    // Release only, as in crates/accshift-egui/build.rs: the stub library's
    // search path would also reach the debug test links.
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        static_vcruntime();
    }
    // The installer's config names the product; the workspace version is
    // checked against it by a test.
    let conf_path = std::path::Path::new("..")
        .join("accshift-egui")
        .join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", conf_path.display());
    let conf: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&conf_path).expect("tauri.conf.json"))
            .expect("tauri.conf.json");
    let product = conf["productName"].as_str().expect("productName");
    let identifier = conf["identifier"].as_str().expect("identifier");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");

    let parts: Vec<u64> = version
        .split('.')
        .map(|p| p.parse().expect("numeric version"))
        .collect();
    let packed = (parts[0] << 48) | (parts[1] << 32) | (parts[2] << 16);
    let mut res = tauri_winres::WindowsResource::new();
    res.set_version_info(tauri_winres::VersionInfo::FILEVERSION, packed);
    res.set_version_info(tauri_winres::VersionInfo::PRODUCTVERSION, packed);
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    res.set("ProductName", product);
    res.set(
        "CompanyName",
        identifier.split('.').nth(1).unwrap_or(identifier),
    );
    res.set("FileDescription", &format!("{product} CLI"));
    res.set("OriginalFilename", "accshift.exe");
    res.compile().expect("Windows resources");
}

/// Copy of `static_vcruntime` in crates/accshift-egui/build.rs (tauri-build's
/// `static_vcruntime::build`): vcruntime linked in, the UCRT left to Windows.
/// Keep both copies identical.
fn static_vcruntime() {
    // The hard-coded msvcrt.lib, replaced by a (mostly) empty object file.
    let machine: &[u8] = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => &[0x64, 0x86],
        Ok("x86") => &[0x4C, 0x01],
        _ => return,
    };
    let bytes: &[u8] = &[
        1, 0, 94, 3, 96, 98, 60, 0, 0, 0, 1, 0, 0, 0, 0, 0, 132, 1, 46, 100, 114, 101, 99, 116,
        118, 101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 10, 16, 0, 46, 100, 114, 101, 99, 116, 118, 101, 0, 0, 0, 0, 1, 0, 0, 0, 3, 0, 4, 0,
        0, 0,
    ];
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let lib = std::path::Path::new(&out_dir).join("msvcrt.lib");
    std::fs::write(&lib, [machine, bytes].concat()).expect("msvcrt.lib");
    println!("cargo:rustc-link-search=native={out_dir}");
    for lib in [
        "libvcruntimed.lib",
        "vcruntime.lib",
        "vcruntimed.lib",
        "libcmtd.lib",
        "msvcrt.lib",
        "msvcrtd.lib",
        "libucrt.lib",
        "libucrtd.lib",
    ] {
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:{lib}");
    }
    for lib in ["libcmt.lib", "libvcruntime.lib", "ucrt.lib"] {
        println!("cargo:rustc-link-arg=/DEFAULTLIB:{lib}");
    }
}
