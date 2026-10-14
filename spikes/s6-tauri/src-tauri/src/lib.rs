use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};
use tauri::{ipc::{InvokeBody, Request, Response}, AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_fs::FsExt;

#[derive(Default)]
struct Folder(Mutex<Option<PathBuf>>);

fn cfg_file(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d.join("folder.json"))
}

fn remember(app: &AppHandle, st: &State<Folder>, p: &Path) -> Result<(), String> {
    fs::write(cfg_file(app)?, serde_json::json!({ "folder": p }).to_string()).map_err(|e| e.to_string())?;
    // also grant the plugin-fs scope at runtime (persisted-scope plugin saves it across restarts)
    let _ = app.fs_scope().allow_directory(p, true);
    *st.0.lock().unwrap() = Some(p.to_path_buf());
    Ok(())
}

/// Relative path inside the chosen folder; rejects absolute, `..`, drive prefixes.
fn resolve(st: &State<Folder>, rel: &str) -> Result<PathBuf, String> {
    let root = st.0.lock().unwrap().clone().ok_or("no folder chosen")?;
    let rp = Path::new(rel);
    if rel.is_empty() || !rp.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("bad path: {rel}"));
    }
    Ok(root.join(rp))
}

#[tauri::command]
fn ping() -> String {
    "pong".into()
}

#[tauri::command]
fn log_line(app: AppHandle, line: String) {
    let p = std::env::var("S6_LOG").map(PathBuf::from).unwrap_or_else(|_| {
        app.path().app_local_data_dir().unwrap_or_else(|_| PathBuf::from(".")).join("s6.log")
    });
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{line}");
    }
}

#[tauri::command]
fn get_folder(st: State<Folder>) -> Option<String> {
    st.0.lock().unwrap().as_ref().map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn pick_folder(app: AppHandle, st: State<'_, Folder>) -> Result<Option<String>, String> {
    let Some(fp) = app.dialog().file().set_title("Choose viSHion data folder").blocking_pick_folder() else {
        return Ok(None);
    };
    let p = fp.into_path().map_err(|e| e.to_string())?;
    remember(&app, &st, &p)?;
    Ok(Some(p.to_string_lossy().into_owned()))
}

/// TEST HOOK ONLY (enabled by env S6_TEST=1): lets automation set a folder without the native dialog.
#[tauri::command]
fn set_folder(app: AppHandle, st: State<Folder>, path: String) -> Result<(), String> {
    if std::env::var("S6_TEST").is_err() {
        return Err("disabled".into());
    }
    remember(&app, &st, Path::new(&path))
}

#[tauri::command]
fn fs_list(st: State<Folder>, rel: Option<String>) -> Result<Vec<String>, String> {
    let root = st.0.lock().unwrap().clone().ok_or("no folder chosen")?;
    let dir = match rel.as_deref() {
        None | Some("") => root,
        Some(r) => resolve(&st, r)?,
    };
    let mut v = vec![];
    for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
        v.push(e.map_err(|e| e.to_string())?.file_name().to_string_lossy().into_owned());
    }
    v.sort();
    Ok(v)
}

#[tauri::command]
fn fs_read(st: State<Folder>, rel: String) -> Result<Response, String> {
    let p = resolve(&st, &rel)?;
    Ok(Response::new(fs::read(p).map_err(|e| e.to_string())?)) // raw binary, no JSON number array
}

/// Atomic write: temp file in same dir + fsync + rename. Body is raw bytes, path in `x-path` header.
#[tauri::command]
fn fs_write(st: State<Folder>, req: Request) -> Result<(), String> {
    let rel = req.headers().get("x-path").and_then(|v| v.to_str().ok()).ok_or("missing x-path")?;
    let InvokeBody::Raw(bytes) = req.body() else { return Err("expected raw body".into()) };
    let dst = resolve(&st, rel)?;
    if let Some(d) = dst.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = dst.with_file_name(format!(".{}.tmp-{}", dst.file_name().unwrap().to_string_lossy(), std::process::id()));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, &dst).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

#[tauri::command]
fn fs_remove(st: State<Folder>, rel: String) -> Result<(), String> {
    fs::remove_file(resolve(&st, &rel)?).map_err(|e| e.to_string())
}

/// "Save as" export: native save dialog then write raw bytes. Returns chosen path or None if cancelled.
#[tauri::command]
async fn save_export(app: AppHandle, req: Request<'_>) -> Result<Option<String>, String> {
    let name = req.headers().get("x-name").and_then(|v| v.to_str().ok()).unwrap_or("export.bin").to_string();
    let InvokeBody::Raw(bytes) = req.body() else { return Err("expected raw body".into()) };
    let Some(fp) = app.dialog().file().set_file_name(&name).blocking_save_file() else {
        return Ok(None);
    };
    let p = fp.into_path().map_err(|e| e.to_string())?;
    fs::write(&p, bytes).map_err(|e| e.to_string())?;
    Ok(Some(p.to_string_lossy().into_owned()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_persisted_scope::init())
        .manage(Folder::default())
        .setup(|app| {
            let h = app.handle().clone();
            // window is created here (config has "create": false) so drag-drop can be toggled per run
            let cfg = h.config().app.windows[0].clone();
            let mut wb = tauri::WebviewWindowBuilder::from_config(&h, &cfg)?;
            if std::env::var("S6_NO_DND").is_ok() {
                wb = wb.disable_drag_drop_handler();
            }
            wb.build()?;
            if let Ok(f) = cfg_file(&h) {
                if let Ok(s) = fs::read_to_string(f) {
                    if let Some(p) = serde_json::from_str::<serde_json::Value>(&s)
                        .ok()
                        .and_then(|v| v["folder"].as_str().map(PathBuf::from))
                    {
                        let st = app.state::<Folder>();
                        *st.0.lock().unwrap() = Some(p.clone());
                        // S6_NO_REALLOW: test switch to prove tauri-plugin-persisted-scope alone restores scope
                        if std::env::var("S6_NO_REALLOW").is_err() {
                            let _ = h.fs_scope().allow_directory(&p, true);
                        }
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping, log_line, get_folder, pick_folder, set_folder, fs_list, fs_read, fs_write, fs_remove, save_export
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
