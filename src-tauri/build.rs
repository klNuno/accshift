fn main() {
    // The Windows launcher starts WebView2 in the data directory tauri will
    // pick for the main window, which is named after the identifier.
    let config = std::fs::read_to_string("tauri.conf.json").expect("read tauri.conf.json");
    let config: serde_json::Value = serde_json::from_str(&config).expect("parse tauri.conf.json");
    let identifier = config["identifier"]
        .as_str()
        .expect("tauri.conf.json has an identifier");
    println!("cargo:rustc-env=ACCSHIFT_IDENTIFIER={identifier}");
    println!("cargo:rerun-if-changed=tauri.conf.json");

    tauri_build::build()
}
