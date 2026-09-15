//! Contract for the main window's Ctrl/Cmd +/- webview zoom.
//!
//! The zoom hotkeys are provided by the Tauri runtime, so the repository-owned
//! contract is narrow: the main window opts in, and the default capability
//! admits the zoom command that the injected hotkey script calls.

use std::fs;
use std::path::Path;

fn read_json(file: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    let contents =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{file} must be readable: {error}"));
    serde_json::from_str(&contents)
        .unwrap_or_else(|error| panic!("{file} must be valid JSON: {error}"))
}

#[test]
fn main_window_enables_webview_zoom_hotkeys() {
    let config = read_json("tauri.conf.json");
    let main_window = config["app"]["windows"]
        .as_array()
        .and_then(|windows| windows.first())
        .expect("main window config must be present");

    assert_eq!(
        main_window["zoomHotkeysEnabled"],
        serde_json::json!(true),
        "main window must enable Ctrl/Cmd +/- webview zoom hotkeys"
    );
}

#[test]
fn main_capability_admits_webview_zoom_command() {
    let capability = read_json("capabilities/default.json");
    let permissions = capability["permissions"]
        .as_array()
        .expect("main capability permissions must be an array");

    assert!(
        permissions
            .iter()
            .any(|permission| permission == "core:webview:allow-set-webview-zoom"),
        "main window must explicitly admit the webview zoom command"
    );
}
