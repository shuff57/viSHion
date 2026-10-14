//! Entry points. Native uses the `vishion` binary (src/main.rs); the web build exports `start(canvas_id)`.
//! This crate stays thin: behaviour lives in core/store/net/ui. What it owns is the platform wiring: which
//! store to open (native folder, browser IndexedDB, or the in-memory demo) and the executor that runs the
//! UI's commands against it.

pub mod backend;
pub mod demo;
pub mod pictures;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod rig;

/// Query-string options of the web build, as `key=value` pairs. Pure so it is testable natively.
pub fn parse_query(search: &str) -> Vec<(String, String)> {
    search
        .trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (p.to_string(), String::new()),
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
mod web {
    use crate::backend::{AnyStore, Backend};
    use std::rc::Rc;
    use vishion_store::idb::IdbStore;
    use vishion_ui::{Cmd, Event, Inbox};
    use wasm_bindgen::prelude::*;

    async fn fetch_bytes(url: &str) -> Option<Vec<u8>> {
        use wasm_bindgen::JsCast;
        let w = web_sys::window()?;
        let r: web_sys::Response = wasm_bindgen_futures::JsFuture::from(w.fetch_with_str(url)).await.ok()?.dyn_into().ok()?;
        if !r.ok() {
            return None;
        }
        let buf = wasm_bindgen_futures::JsFuture::from(r.array_buffer().ok()?).await.ok()?;
        Some(js_sys::Uint8Array::new(&buf).to_vec())
    }

    /// `localStorage["ocr_settings_v1"]`, the JS app's own copy of the settings (theme, demo, API key, topics...).
    mod local_settings {
        use vishion_core::model::Settings;
        const KEY: &str = "ocr_settings_v1";

        fn storage() -> Option<web_sys::Storage> {
            web_sys::window()?.local_storage().ok()?
        }

        /// `Object.assign({}, DEFAULTS, saved)` over whatever `settings.json` gave: a key kept in this browser wins.
        pub fn overlay(settings: &mut Settings) {
            let Some(text) = storage().and_then(|s| s.get_item(KEY).ok().flatten()) else { return };
            let Ok(saved) = serde_json::from_str::<serde_json::Value>(&text) else { return };
            if let serde_json::Value::Object(m) = &saved {
                for (k, v) in m {
                    settings.set(k, v.clone());
                }
            }
            settings.set("defaultPrompt", serde_json::Value::String(vishion_core::model::DEFAULT_PROMPT.into()));
        }

        /// One key of the saved settings (the JS app keeps `storage: "idb"` here when the person chose the browser).
        pub fn get_str(key: &str) -> Option<String> {
            let text = storage()?.get_item(KEY).ok().flatten()?;
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            v.get(key)?.as_str().map(String::from)
        }

        pub fn set_str(key: &str, value: &str) {
            let Some(st) = storage() else { return };
            let mut v: serde_json::Value = st
                .get_item(KEY)
                .ok()
                .flatten()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(o) = v.as_object_mut() {
                o.insert(key.into(), serde_json::json!(value));
            }
            if let Ok(t) = serde_json::to_string(&v) {
                let _ = st.set_item(KEY, &t);
            }
        }

        pub fn store(settings: &Settings) {
            if let (Some(st), Ok(text)) = (storage(), serde_json::to_string(&settings.0)) {
                let _ = st.set_item(KEY, &text);
            }
        }
    }

    /// What the storage card of Home says (js/fs.js `S.d.folder`): the remembered folder and whether the
    /// browser lets us use it without asking again.
    pub async fn folder_info() -> vishion_ui::FolderInfo {
        use vishion_store::fsa::{supported, FsaStore};
        let supported = supported();
        let idb = IdbStore::open().await.ok();
        let fsa = match &idb {
            Some(i) => FsaStore::from_ocr_fs(i).await.ok().flatten(),
            None => None,
        };
        match fsa {
            Some(f) => {
                let permission = f.permission().await.unwrap_or_else(|_| "prompt".into());
                vishion_ui::FolderInfo { mode: "folder".into(), name: f.name(), permission: if permission == "granted" { "granted".into() } else { "prompt".into() }, supported }
            }
            None => {
                let chose_browser = local_settings::get_str("storage").as_deref() == Some("idb");
                let mode = if supported && !chose_browser { "none" } else { "idb" };
                vishion_ui::FolderInfo { mode: mode.into(), name: String::new(), permission: "granted".into(), supported }
            }
        }
    }

    /// The Home card's three actions. Each ends by reloading the page, so the app starts on the new store.
    async fn folder_command(what: &str) -> Option<String> {
        use vishion_store::fsa::{pick_directory, FsaStore};
        let idb = IdbStore::open().await.ok()?;
        match what {
            "choose" => match pick_directory().await {
                Ok(Some(h)) => {
                    idb.kv_set("root", &h).await.ok()?;
                    local_settings::set_str("storage", "auto");
                    reload();
                    None
                }
                Ok(None) => None,
                Err(e) => Some(format!("Could not use that folder: {e}")),
            },
            "browser" => {
                idb.kv_delete("root").await.ok()?;
                local_settings::set_str("storage", "idb");
                reload();
                None
            }
            "reconnect" => {
                let f = FsaStore::from_ocr_fs(&idb).await.ok().flatten()?;
                match f.request_permission().await {
                    Ok(p) if p == "granted" => {
                        reload();
                        None
                    }
                    _ => Some("The folder was not reconnected. Choose it again, or use browser storage.".into()),
                }
            }
            _ => None,
        }
    }

    fn reload() {
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
    }

    /// Mount the egui app on the canvas with the given id.
    ///
    /// URL options: `?demo=1` runs the sample assignment from memory (nothing is stored), `?project=<slug>`
    /// opens that assignment instead of the most recent one, `?theme=light|dark` forces a theme, `?view=inbox` opens the assignment overview.
    #[wasm_bindgen]
    pub async fn start(canvas_id: String) -> Result<(), JsValue> {
        eframe::WebLogger::init(log::LevelFilter::Warn).ok();
        let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
        let q = crate::parse_query(&search);
        let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        let demo = get("demo").is_some_and(|v| v != "0");
        let want = get("project");
        let theme = get("theme").filter(|t| t == "light" || t == "dark");
        let inbox_first = get("view").is_some_and(|v| v == "inbox");
        let page = get("page");
        let no_tray = get("tray").is_some_and(|v| v == "0");

        let now = || js_sys::Date::now() as i64;
        let backend: Rc<Backend> = Rc::new(if demo {
            let (store, _) = crate::demo::seed(now()).await.map_err(|e| JsValue::from_str(&e.to_string()))?;
            Backend::new_demo(store, "Demo assignment (not saved)")
        } else {
            let idb = IdbStore::open().await.map_err(|e| JsValue::from_str(&e.to_string()))?;
            // a remembered folder with permission still granted is the store; otherwise the browser's own
            match vishion_store::fsa::FsaStore::from_ocr_fs(&idb).await {
                Ok(Some(f)) if f.permission().await.ok().as_deref() == Some("granted") => Backend::new(AnyStore::Fsa(f), "Saved to your folder"),
                _ => Backend::new(AnyStore::Idb(idb), "Saved in this browser"),
            }
        });

        eframe::WebRunner::new()
            .start(
                &canvas_id,
                eframe::WebOptions::default(),
                Box::new(move |cc| {
                    let inbox = Inbox::new(cc.egui_ctx.clone());
                    let dispatch: vishion_ui::Dispatch = {
                        let (b, inbox) = (backend.clone(), inbox.clone());
                        Box::new(move |cmd: Cmd| {
                            if matches!(cmd, Cmd::Stop) {
                                b.cancel.stop();
                                return;
                            }
                            let (b, inbox) = (b.clone(), inbox.clone());
                            wasm_bindgen_futures::spawn_local(async move {
                                if matches!(cmd, Cmd::LoadMathFont) {
                                    // the 840 KB math font is a separate file, fetched when a page first has math
                                    let bytes = fetch_bytes("stix-two-math.otf").await.unwrap_or_default();
                                    inbox.push(Event::MathFont(bytes));
                                    return;
                                }
                                if matches!(cmd, Cmd::LoadEmojiFont) {
                                    // colour emoji: a 1.4 MB COLR face, fetched only for a page that has an emoji
                                    let bytes = fetch_bytes("twemoji-colr.ttf").await.unwrap_or_default();
                                    inbox.push(Event::EmojiFont(bytes));
                                    return;
                                }
                                if matches!(cmd, Cmd::LoadFallbackFont) {
                                    // CJK, Hangul and emoji: a 4 MB file fetched only for a page that has such text
                                    let bytes = fetch_bytes("fallback-cjk-emoji.ttf").await.unwrap_or_default();
                                    inbox.push(Event::FallbackFont(bytes));
                                    return;
                                }
                                // js/state.js `saveSettings`: the whole settings object, API key included, is kept in this
                                // browser as `ocr_settings_v1`; `settings.json` (secrets stripped) is the backend's part
                                if let Cmd::Folder(what) = &cmd {
                                    if let Some(msg) = folder_command(what).await {
                                        inbox.push(Event::Notice { text: msg, error: true });
                                    }
                                    return;
                                }
                                if matches!(cmd, Cmd::ListProjects) {
                                    inbox.push(Event::Folder(folder_info().await));
                                }
                                if let Cmd::SaveSettings { settings } = &cmd {
                                    local_settings::store(settings);
                                }
                                if let Cmd::Transcribe { slug, ids, conn } = cmd {
                                    b.read_pages(&slug, ids, conn, &|e| inbox.push(e)).await;
                                    return;
                                }
                                if let Cmd::StagePictures { conn, read_names, preset } = cmd {
                                    b.stage_pictures(&conn, read_names, preset.as_deref(), &|e| inbox.push(e)).await;
                                    return;
                                }
                                let e: Event = b.handle(cmd, js_sys::Date::now() as i64).await;
                                inbox.push(e);
                            });
                        })
                    };
                    {
                        let (b, inbox) = (backend.clone(), inbox.clone());
                        wasm_bindgen_futures::spawn_local(async move {
                            let mut e = b.open(want.as_deref(), js_sys::Date::now() as i64).await;
                            // js/state.js `loadSettings`: this browser's `ocr_settings_v1` is the settings of the app
                            if let Event::Opened { settings, .. } = &mut e {
                                local_settings::overlay(settings);
                            }
                            inbox.push(e);
                        });
                    }
                    let mut app = vishion_ui::App::new(cc.egui_ctx.clone()).with_host(dispatch, inbox).inbox(inbox_first).no_tray(no_tray);
                    if let Some(t) = theme {
                        app = app.theme(&t);
                    }
                    if let Some(p) = &page {
                        app = app.page(p);
                    }
                    Ok(Box::new(app))
                }),
            )
            .await?;
        // eframe sets tabindex=0 on the canvas and appends an unnamed IME <input>; fix both (keyboard path).
        vishion_ui::harden_web_dom(&canvas_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_query;

    #[test]
    fn query_pairs() {
        assert_eq!(
            parse_query("?demo=1&project=unit-5&flag"),
            vec![("demo".into(), "1".into()), ("project".into(), "unit-5".into()), ("flag".into(), "".into())]
        );
        assert!(parse_query("").is_empty());
    }
}
