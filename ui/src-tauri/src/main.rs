#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use ox_core::{store::Store, Session};
use serde_json::Value;
use std::sync::{Arc, Mutex};

struct Slot { session: Arc<Mutex<Session>>, budget: ox_core::spill::CodeBudget }

// async = runs off the UI thread, so importing a big file doesn't freeze the window;
// `processes` and `cancel_process` skip the session lock so they answer during a long command
#[tauri::command]
async fn ox(slot: tauri::State<'_, Slot>, cmd: String, args: Value) -> Result<Value, String> {
    let session = slot.session.clone();
    let budget = slot.budget.clone();
    tauri::async_runtime::spawn_blocking(move || ox_core::storage::with_budget(budget, || {
        ox_core::dispatch_free(&cmd, &args).unwrap_or_else(|| ox_core::dispatch(&mut session.lock().unwrap_or_else(|e| e.into_inner()), &cmd, args))
    })).await.map_err(|error| format!("The command worker failed: {error}"))?
}

fn main() {
    let store = Store::open(Store::default_root()).expect("cannot create the OxRefine data folder");
    let budget = store.code_budget();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Slot { session: Arc::new(Mutex::new(Session::with_store(store))), budget })
        .invoke_handler(tauri::generate_handler![ox])
        .run(tauri::generate_context!())
        .expect("failed to start OxRefine");
}
