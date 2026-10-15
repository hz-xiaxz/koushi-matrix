// #1436: a release build must not open a console window next to the app.
// Debug builds keep it so their logs stay visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    koushi_desktop::run();
}
