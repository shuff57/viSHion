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

        pub fn store(settings: &Settings) {
            if let (Some(st), Ok(text)) = (storage(), serde_json::to_string(&settings.0)) {
                let _ = st.set_item(KEY, &text);
            }
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
            Backend::new(AnyStore::Idb(idb), "Saved in this browser")
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
