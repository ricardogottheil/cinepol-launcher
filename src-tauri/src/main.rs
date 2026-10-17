// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = cinepol_launcher_lib::run() {
        eprintln!("Cinepol Launcher failed to start: {error}");
        std::process::exit(1);
    }
}
