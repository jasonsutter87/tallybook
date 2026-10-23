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
    // `Tallybook --mcp [--write] [--book <path>]`: no window, just the books on
    // stdin/stdout for an agent (see docs/AGENTS.md).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--mcp") {
        return serve_mcp(args);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Signed self-updates from GitHub Releases (see docs/RELEASING.md).
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            // Development builds keep their own recent list and keychain entries,
            // so testing never touches the installed app's books or bank keys.
            let (config_dir, service) = if cfg!(debug_assertions) {
                (app.path().app_config_dir()?.join("dev"), "app.tallybook.dev")
            } else {
                (app.path().app_config_dir()?, "app.tallybook")
            };
            app.manage(Arc::new(App::new(Arc::new(Keychain::new(service)), config_dir)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![call])
        .run(tauri::generate_context!())
        .expect("error while running Tallybook");
}

/// Runs the MCP server with the same config dir and keychain this build of the app uses.
/// Arguments are parsed in order by the same parser as `tallybook-mcp`.
fn serve_mcp(args: Vec<String>) {
    let args = match tally_mcp::parse_args(args) {
        Ok(a) if !a.dev && a.config_dir.is_none() && !a.help => a,
        Ok(_) => {
            eprintln!("Tallybook --mcp takes only --write and --book <path>; use tallybook-mcp for more options");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("Tallybook --mcp: {e}");
            std::process::exit(2);
        }
    };
    let opts = tally_mcp::Options { write: args.write, book: args.book };
    let (config_dir, service) = if cfg!(debug_assertions) {
        (tally_mcp::desktop_config_dir().join("dev"), "app.tallybook.dev")
    } else {
        (tally_mcp::desktop_config_dir(), "app.tallybook")
    };
    let app = Arc::new(App::new(Arc::new(Keychain::new(service)), config_dir));
    if let Err(e) = tauri::async_runtime::block_on(tally_mcp::serve_stdio(app, opts)) {
        eprintln!("Tallybook --mcp: {e}");
        std::process::exit(1);
    }
}
