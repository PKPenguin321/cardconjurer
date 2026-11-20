// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod cards;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cards::CardStore;
use tauri::http::{header, Response};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

/// The app's own URL scheme. WebView2 on Windows exposes custom schemes as
/// `http://<scheme>.localhost/`; other platforms use `<scheme>://localhost/`.
const SCHEME: &str = "cc";

type CommandResult<T> = Result<T, String>;

#[tauri::command]
fn cards_list(store: State<'_, CardStore>) -> CommandResult<Vec<String>> {
    store.list().map_err(|e| e.to_string())
}

#[tauri::command]
fn cards_get(store: State<'_, CardStore>, key: String) -> CommandResult<Option<String>> {
    store.get(&key).map_err(|e| e.to_string())
}

#[tauri::command]
fn cards_set(store: State<'_, CardStore>, key: String, data: String) -> CommandResult<()> {
    store.set(&key, &data).map_err(|e| e.to_string())
}

#[tauri::command]
fn cards_remove(store: State<'_, CardStore>, key: String) -> CommandResult<()> {
    store.remove(&key).map_err(|e| e.to_string())
}

#[tauri::command]
fn cards_clear(store: State<'_, CardStore>) -> CommandResult<()> {
    store.clear().map_err(|e| e.to_string())
}

/// Where saved cards live, so the UI can tell the user.
#[tauri::command]
fn cards_folder(store: State<'_, CardStore>) -> String {
    store.dir().display().to_string()
}

fn main() {
    let exe = std::env::current_exe().ok();
    let build_repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let asset_root: Arc<Option<PathBuf>> =
        Arc::new(assets::find_root(exe.as_deref(), &build_repo).map(|p| p.canonicalize().unwrap_or(p)));
    match asset_root.as_ref() {
        Some(root) => println!("Serving CardConjurer files from {}", root.display()),
        None => eprintln!("CardConjurer files not found; set {}", assets::ASSETS_ENV_VAR),
    }

    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol(SCHEME, move |_ctx, request, responder| {
            let root = Arc::clone(&asset_root);
            let path = request.uri().path().to_string();
            // Read files off the UI thread; frame images can be several MB each.
            tauri::async_runtime::spawn_blocking(move || {
                let asset = assets::serve(root.as_ref().as_deref(), &path);
                let response = Response::builder()
                    .status(asset.status)
                    .header(header::CONTENT_TYPE, asset.content_type)
                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                    .body(asset.body)
                    .expect("valid response");
                responder.respond(response);
            });
        })
        .invoke_handler(tauri::generate_handler![
            cards_list,
            cards_get,
            cards_set,
            cards_remove,
            cards_clear,
            cards_folder
        ])
        .setup(|app| {
            let cards_dir = app.path().app_data_dir()?.join("cards");
            app.manage(CardStore::new(cards_dir)?);

            let start_url = if cfg!(windows) {
                format!("http://{SCHEME}.localhost/")
            } else {
                format!("{SCHEME}://localhost/")
            };
            WebviewWindowBuilder::new(app, "main", WebviewUrl::CustomProtocol(start_url.parse()?))
                .title("Card Conjurer")
                .inner_size(1440.0, 960.0)
                .min_inner_size(900.0, 600.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Card Conjurer");
}
