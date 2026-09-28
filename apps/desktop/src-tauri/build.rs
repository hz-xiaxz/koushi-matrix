fn main() {
    // One named compilation boundary for the desktop updater (#1035): the
    // shared lifecycle engine is compiled on every platform, while
    // `koushi_updater_backend` marks targets that ship a native install
    // backend. Only macOS has one today; a future Linux backend enables this
    // cfg here instead of adding platform attributes to shared updater code.
    println!("cargo::rustc-check-cfg=cfg(koushi_updater_backend)");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo::rustc-cfg=koushi_updater_backend");
    }
    tauri_build::build()
}
