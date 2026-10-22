fn main() {
    // The Mermaid version is part of the SVG cache key (H3): a new Mermaid
    // renders every diagram again.
    let pkg = "../ui/node_modules/mermaid/package.json";
    println!("cargo:rerun-if-changed={pkg}");
    let version = std::fs::read_to_string(pkg)
        .ok()
        .and_then(|s| s.split("\"version\"").nth(1).and_then(|r| r.split('"').nth(1)).map(String::from))
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=MAIDO_MERMAID_VERSION={version}");
    tauri_build::build()
}
