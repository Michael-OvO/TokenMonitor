fn main() {
    tauri_build::build();

    // wry/tao statically import comctl32 v6 APIs (SetWindowSubclass,
    // TaskDialogIndirect, ...). tauri_build embeds the Common-Controls v6
    // activation manifest only into the app binary, so a `cargo test` harness
    // binary binds to comctl32 v5 and fails to load on Windows with 0xC0000139
    // STATUS_ENTRYPOINT_NOT_FOUND (the whole suite can't run on Windows).
    //
    // Cargo has no link-arg key that targets only the lib unit-test binary:
    // `rustc-link-arg-tests` covers only `[[test]]` integration tests, while
    // `rustc-link-arg` would also hit the app bin, which already carries a
    // manifest from tauri_build -> CVT1100 "duplicate resource". So gate on an
    // env var that the CI test step sets and run `cargo test --lib` there (which
    // never builds the bin). Release builds (`cargo build` / `tauri build`) do
    // not set the var, so the app binary is untouched.
    println!("cargo:rerun-if-env-changed=TM_EMBED_TEST_MANIFEST");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if std::env::var_os("TM_EMBED_TEST_MANIFEST").is_some()
        && target_os == "windows"
        && target_env == "msvc"
    {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("resources/windows-test.manifest");
        println!("cargo:rerun-if-changed=resources/windows-test.manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
