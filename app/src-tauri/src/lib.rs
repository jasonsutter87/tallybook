//! The desktop shell is deliberately thin: one IPC command that forwards
//! to `tally_api::App::call`, the same dispatcher the dev server uses.

use std::sync::Arc;
use tally_api::{ApiError, App, Keychain};
use tauri::Manager;

#[tauri::command]
async fn call(app: tauri::State<'_, Arc<App>>, method: String, params: serde_json::Value) -> Result<serde_json::Value, ApiError> {
    app.call(&method, params).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            app.manage(Arc::new(App::new(Arc::new(Keychain::new("app.tallybook")), config_dir)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![call])
        .run(tauri::generate_context!())
        .expect("error while running Tallybook");
}
