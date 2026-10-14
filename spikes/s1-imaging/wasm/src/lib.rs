use wasm_bindgen::prelude::*;

/// Flat [x,y,w,h,r,s] * n, all f64 (same as the JS band objects).
#[wasm_bindgen(js_name = lineBands)]
pub fn line_bands(rgba: &[u8], width: u32, height: u32) -> Vec<f64> {
    imaging_core::line_bands(rgba, width as usize, height as usize)
}

#[wasm_bindgen(js_name = skewAngle)]
pub fn skew_angle(rgba: &[u8], width: u32, height: u32) -> f64 {
    imaging_core::skew_angle(rgba, width as usize, height as usize)
}

/// rgba is the 16x16 downsample; returns [r,g,b].
#[wasm_bindgen(js_name = edgeColor)]
pub fn edge_color(rgba: &[u8]) -> Vec<u8> {
    imaging_core::edge_color(rgba).to_vec()
}

/// Debug: [sin, cos] of deg, as JS's skew `score` computes them.
#[wasm_bindgen(js_name = trig)]
pub fn trig(deg: f64) -> Vec<f64> {
    let r = (deg * std::f64::consts::PI) / 180.0;
    vec![r.sin(), r.cos()]
}
