// Windows: the version resource the GUI also carries, with the same product
// name and version. Code signing restricts signed files to these values.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !(windows && msvc) {
        return;
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
