// Windows startup: the loader maps every statically imported DLL before
// `main`. A bare Rust exe reaches `main` in about 13 ms here; this one took 31.
// The DLLs below are only needed for accessibility clients (UI Automation),
// links and COM, never for the first frame, so they load on first call.
//
// The exe also gets what `tauri build` gives `accshift-gui.exe`: the VC
// runtime linked statically, and its icon, version and manifest resources.
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
    static_vcruntime();
    resources();
}

/// tauri-build's `static_vcruntime::build`, which the Tauri CLI turns on
/// for every build (`STATIC_VCRUNTIME=true`): vcruntime linked in, the UCRT
/// left to Windows, so the exe starts without the VC++ redistributable.
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

/// tauri-build's resource file, from the same tauri.conf.json: the icon as
/// resource 32512, the version info and the Common Controls 6 manifest.
fn resources() {
    let tauri = std::path::Path::new("../../src-tauri");
    let conf_path = tauri.join("tauri.conf.json");
    let icon = tauri.join("icons").join("icon.ico");
    println!("cargo:rerun-if-changed={}", conf_path.display());
    println!("cargo:rerun-if-changed={}", icon.display());
    let conf: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&conf_path).expect("tauri.conf.json"))
            .expect("tauri.conf.json");
    let text = |key: &str| conf[key].as_str().expect(key).to_string();
    let (version, product, identifier) = (text("version"), text("productName"), text("identifier"));
    let mut res = tauri_winres::WindowsResource::new();
    res.set_manifest(MANIFEST);
    // `to_winres_version`: no build metadata in the app's versions.
    let parts: Vec<u64> = version
        .split('.')
        .map(|p| p.parse().expect("numeric version"))
        .collect();
    let packed = (parts[0] << 48) | (parts[1] << 32) | (parts[2] << 16);
    res.set_version_info(tauri_winres::VersionInfo::FILEVERSION, packed);
    res.set_version_info(tauri_winres::VersionInfo::PRODUCTVERSION, packed);
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    res.set("ProductName", &product);
    // No `bundle.publisher`: the identifier's second part.
    res.set(
        "CompanyName",
        identifier.split('.').nth(1).unwrap_or(&identifier),
    );
    res.set("FileDescription", &product);
    res.set_icon_with_id(&icon.display().to_string(), "32512");
    res.compile().expect("Windows resources");
}

/// tauri-build's windows-app-manifest.xml.
const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;
