/// Tauri refuses to build when a sidecar listed in `bundle.externalBin` is
/// missing, which would break `cargo test` and `cargo clippy`. The real shim is
/// copied in by `scripts/sidecar.mjs` before `tauri dev` and `tauri build`;
/// until then an empty placeholder stands in, and the app treats an empty
/// shim as absent.
fn ensure_sidecar_placeholder() {
    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("binaries");
    let target = std::env::var("TARGET").unwrap();
    let exe = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = dir.join(format!("agentscope-hook-{target}{exe}"));
    if !path.exists() {
        std::fs::create_dir_all(&dir).expect("failed to create binaries/");
        std::fs::write(&path, b"").expect("failed to write the sidecar placeholder");
        println!("cargo:warning=agentscope-hook sidecar missing; wrote an empty placeholder. Run `pnpm sidecar` for a working shim.");
    }
}

fn main() {
    ensure_sidecar_placeholder();
    // tauri-build embeds the Windows app manifest (Common Controls v6) into
    // binaries only, so test executables that link Tauri fail to start with
    // STATUS_ENTRYPOINT_NOT_FOUND. Embed it ourselves for every linked target.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let mut windows = tauri_build::WindowsAttributes::new();
    if target_os == "windows" && target_env == "msvc" {
        windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");
}
