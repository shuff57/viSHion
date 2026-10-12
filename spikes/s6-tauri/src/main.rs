use leptos::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // window.__TAURI__.core.invoke  (withGlobalTauri = true)
    #[wasm_bindgen(js_namespace = ["__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

#[component]
fn App() -> impl IntoView {
    let (msg, set_msg) = signal(String::from("(not called)"));
    leptos::task::spawn_local(async move {
        let r = invoke("ping", JsValue::NULL).await;
        set_msg.set(match r {
            Ok(v) => format!("wasm->tauri ok: {}", v.as_string().unwrap_or_default()),
            Err(e) => format!("wasm->tauri ERR: {:?}", e),
        });
    });
    let origin = web_sys::window().unwrap().location().origin().unwrap_or_default();
    view! {
        <p id="wasm-status">"wasm mounted; origin=" {origin} "; " {msg}</p>
    }
}

fn main() {
    mount_to_body(|| view! { <App/> });
}
