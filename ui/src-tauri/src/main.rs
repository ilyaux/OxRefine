#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use ox_core::{store::Store, Session};
use serde_json::Value;
use std::sync::Mutex;

struct Slot(Mutex<Session>);

// async = runs off the UI thread, so importing a big file doesn't freeze the window;
// `processes` and `cancel_process` skip the session lock so they answer during a long command
#[tauri::command]
async fn ox(slot: tauri::State<'_, Slot>, cmd: String, args: Value) -> Result<Value, String> {
    ox_core::dispatch_free(&cmd, &args).unwrap_or_else(|| ox_core::dispatch(&mut slot.0.lock().unwrap_or_else(|e| e.into_inner()), &cmd, args))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Slot(Mutex::new(Session::with_store(Store::open(Store::default_root()).expect("cannot create the OxRefine data folder")))))
        .invoke_handler(tauri::generate_handler![ox])
        .run(tauri::generate_context!())
        .expect("failed to start OxRefine");
}
