//! The browser's camera for the capture screen (`design/Phone.dc.html`): `getUserMedia` into a hidden
//! `<video>`, a timer that draws it to a canvas and sends the pixels as `Event::CameraFrame`. Pure
//! plumbing: finding the page and keeping a photo are `core::capture` and the UI.
use std::cell::RefCell;
use vishion_ui::Event;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement, MediaStream, MediaStreamConstraints};

/// Frames are sent at most this wide (the detector and the viewfinder do not need more).
const MAX_W: u32 = 960;
/// Milliseconds between frames (about 6 a second).
const EVERY_MS: i32 = 160;

struct Cam {
    stream: MediaStream,
    timer: i32,
    _tick: Closure<dyn FnMut()>,
}

thread_local! {
    static CAM: RefCell<Option<Cam>> = const { RefCell::new(None) };
}

fn err(e: JsValue) -> String {
    let name = js_sys::Reflect::get(&e, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_default();
    match name.as_str() {
        "NotAllowedError" | "SecurityError" => "The camera is blocked. Allow it in the browser, or add photos from files.".into(),
        "NotFoundError" | "OverconstrainedError" => "No camera was found on this device.".into(),
        _ => "The camera could not be opened.".into(),
    }
}

/// Open the camera and push frames until [`stop`].
pub async fn start(push: impl Fn(Event) + 'static) -> Result<(), String> {
    stop();
    let win = web_sys::window().ok_or("No window.")?;
    let doc = win.document().ok_or("No document.")?;
    let md = win.navigator().media_devices().map_err(err)?;
    let video_c = js_sys::Object::new();
    let ideal = |v: JsValue| {
        let o = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&o, &"ideal".into(), &v);
        o
    };
    let _ = js_sys::Reflect::set(&video_c, &"facingMode".into(), &ideal("environment".into()));
    let _ = js_sys::Reflect::set(&video_c, &"width".into(), &ideal(1280.into()));
    let c = MediaStreamConstraints::new();
    c.set_video(&video_c.into());
    let stream: MediaStream = JsFuture::from(md.get_user_media_with_constraints(&c).map_err(err)?).await.map_err(err)?.unchecked_into();

    let video: HtmlVideoElement = doc.create_element("video").map_err(err)?.unchecked_into();
    video.set_muted(true);
    let _ = video.set_attribute("playsinline", "");
    video.set_src_object(Some(&stream));
    JsFuture::from(video.play().map_err(err)?).await.map_err(err)?;
    let canvas: HtmlCanvasElement = doc.create_element("canvas").map_err(err)?.unchecked_into();
    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")
        .map_err(err)?
        .ok_or("No canvas.")?
        .unchecked_into();

    let tick = Closure::<dyn FnMut()>::new(move || {
        let (vw, vh) = (video.video_width(), video.video_height());
        if vw == 0 || vh == 0 {
            return;
        }
        let w = vw.min(MAX_W);
        let h = ((vh as f64) * (w as f64) / (vw as f64)).round() as u32;
        canvas.set_width(w);
        canvas.set_height(h);
        if ctx.draw_image_with_html_video_element_and_dw_and_dh(&video, 0.0, 0.0, w as f64, h as f64).is_err() {
            return;
        }
        if let Ok(img) = ctx.get_image_data(0, 0, w as i32, h as i32) {
            push(Event::CameraFrame { w, h, rgba: img.data().0 });
        }
    });
    let timer = win
        .set_interval_with_callback_and_timeout_and_arguments_0(tick.as_ref().unchecked_ref(), EVERY_MS)
        .map_err(err)?;
    CAM.with(|c| *c.borrow_mut() = Some(Cam { stream, timer, _tick: tick }));
    Ok(())
}

/// Stop the frames and release the camera (its light goes off).
pub fn stop() {
    CAM.with(|c| {
        if let Some(cam) = c.borrow_mut().take() {
            if let Some(w) = web_sys::window() {
                w.clear_interval_with_handle(cam.timer);
            }
            for t in cam.stream.get_tracks().iter() {
                if let Ok(f) = js_sys::Reflect::get(&t, &"stop".into()) {
                    if let Ok(f) = f.dyn_into::<js_sys::Function>() {
                        let _ = f.call0(&t);
                    }
                }
            }
        }
    });
}
