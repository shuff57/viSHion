use leptos::prelude::*;

fn read_theme() -> String {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("ocr_settings_v1").ok().flatten())
        .unwrap_or_else(|| "(none)".to_string())
}

#[component]
fn App() -> impl IntoView {
    let (count, set_count) = signal(0);
    let stored = read_theme();
    view! {
        <main class="s2">
            <h1 style="font-family: Caveat">"viSHion S2"</h1>
            <p id="stored">"stored: " {stored}</p>
            <button id="inc" on:click=move |_| set_count.update(|c| *c += 1)>"clicks"</button>
            <span id="count">{count}</span>
        </main>
    }
}

fn main() {
    mount_to_body(|| view! { <App/> });
    // Startup mark for Playwright TTI measurement.
    if let Some(p) = web_sys::window().and_then(|w| w.performance()) {
        let _ = p.mark("app-mounted");
    }
}
