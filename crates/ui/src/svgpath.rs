//! A tiny SVG path flattener for the line icons (js/ui/icons.js is a set of 24 x 24 stroke paths). Supports
//! M L H V C S Q A Z in both cases; curves and arcs become short polylines. Pure, unit-tested.

/// One sub-path as points on the 24-unit grid, and whether it was closed with `z`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sub {
    pub pts: Vec<[f32; 2]>,
    pub closed: bool,
}

struct Scan<'a> {
    b: &'a [u8],
    i: usize,
}

impl Scan<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && (self.b[self.i] == b' ' || self.b[self.i] == b',' || self.b[self.i] == b'\n') {
            self.i += 1;
        }
    }

    fn more(&mut self) -> bool {
        self.ws();
        self.i < self.b.len() && !self.b[self.i].is_ascii_alphabetic()
    }

    fn num(&mut self) -> f32 {
        self.ws();
        let s = self.i;
        if self.i < self.b.len() && (self.b[self.i] == b'-' || self.b[self.i] == b'+') {
            self.i += 1;
        }
        let mut dot = false;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                self.i += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.b[s..self.i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0)
    }

    /// An arc flag is one character, possibly with no separator before the next number.
    fn flag(&mut self) -> bool {
        self.ws();
        let v = self.i < self.b.len() && self.b[self.i] == b'1';
        self.i += 1;
        v
    }
}

fn arc(out: &mut Vec<[f32; 2]>, from: [f32; 2], rx: f32, ry: f32, rot: f32, large: bool, sweep: bool, to: [f32; 2]) {
    if rx.abs() < 1e-6 || ry.abs() < 1e-6 || from == to {
        out.push(to);
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    let phi = rot.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (from[0] - to[0]) / 2.0;
    let dy = (from[1] - to[1]) / 2.0;
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lam = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lam > 1.0 {
        let k = lam.sqrt();
        rx *= k;
        ry *= k;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1 / ry;
    let cyp = -co * ry * x1 / rx;
    let cx = cp * cxp - sp * cyp + (from[0] + to[0]) / 2.0;
    let cy = sp * cxp + cp * cyp + (from[1] + to[1]) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let t1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dt = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let n = ((dt.abs() / std::f32::consts::FRAC_PI_2) * 6.0).ceil().max(2.0) as usize;
    for k in 1..=n {
        let t = t1 + dt * k as f32 / n as f32;
        let (st, ct) = t.sin_cos();
        out.push([cx + rx * ct * cp - ry * st * sp, cy + rx * ct * sp + ry * st * cp]);
    }
}

fn cubic(out: &mut Vec<[f32; 2]>, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) {
    for k in 1..=10 {
        let t = k as f32 / 10.0;
        let u = 1.0 - t;
        let f = |i: usize| u * u * u * p0[i] + 3.0 * u * u * t * p1[i] + 3.0 * u * t * t * p2[i] + t * t * t * p3[i];
        out.push([f(0), f(1)]);
    }
}

pub fn parse(d: &str) -> Vec<Sub> {
    let mut sc = Scan { b: d.as_bytes(), i: 0 };
    let mut subs: Vec<Sub> = vec![];
    let mut cur = [0f32; 2];
    let mut start = [0f32; 2];
    let mut last_ctrl: Option<[f32; 2]> = None;
    let mut cmd = b'M';
    let mut pts: Vec<[f32; 2]> = vec![];
    let flush = |pts: &mut Vec<[f32; 2]>, subs: &mut Vec<Sub>, closed: bool| {
        if pts.len() > 1 || (closed && !pts.is_empty()) {
            subs.push(Sub { pts: std::mem::take(pts), closed });
        } else {
            pts.clear();
        }
    };
    loop {
        sc.ws();
        if sc.i >= sc.b.len() {
            break;
        }
        if sc.b[sc.i].is_ascii_alphabetic() {
            cmd = sc.b[sc.i];
            sc.i += 1;
            if cmd == b'z' || cmd == b'Z' {
                flush(&mut pts, &mut subs, true);
                cur = start;
                last_ctrl = None;
                continue;
            }
        } else if !sc.more() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (cur[0], cur[1]) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                flush(&mut pts, &mut subs, false);
                let p = [sc.num() + ox, sc.num() + oy];
                cur = p;
                start = p;
                pts.push(p);
                cmd = if rel { b'l' } else { b'L' };
                last_ctrl = None;
            }
            b'L' => {
                let p = [sc.num() + ox, sc.num() + oy];
                cur = p;
                pts.push(p);
                last_ctrl = None;
            }
            b'H' => {
                cur = [sc.num() + ox, cur[1]];
                pts.push(cur);
                last_ctrl = None;
            }
            b'V' => {
                cur = [cur[0], sc.num() + oy];
                pts.push(cur);
                last_ctrl = None;
            }
            b'C' => {
                let p1 = [sc.num() + ox, sc.num() + oy];
                let p2 = [sc.num() + ox, sc.num() + oy];
                let p3 = [sc.num() + ox, sc.num() + oy];
                cubic(&mut pts, cur, p1, p2, p3);
                last_ctrl = Some(p2);
                cur = p3;
            }
            b'S' => {
                let p1 = match last_ctrl {
                    Some(c) => [2.0 * cur[0] - c[0], 2.0 * cur[1] - c[1]],
                    None => cur,
                };
                let p2 = [sc.num() + ox, sc.num() + oy];
                let p3 = [sc.num() + ox, sc.num() + oy];
                cubic(&mut pts, cur, p1, p2, p3);
                last_ctrl = Some(p2);
                cur = p3;
            }
            b'Q' => {
                let q = [sc.num() + ox, sc.num() + oy];
                let p = [sc.num() + ox, sc.num() + oy];
                let c1 = [cur[0] + 2.0 / 3.0 * (q[0] - cur[0]), cur[1] + 2.0 / 3.0 * (q[1] - cur[1])];
                let c2 = [p[0] + 2.0 / 3.0 * (q[0] - p[0]), p[1] + 2.0 / 3.0 * (q[1] - p[1])];
                cubic(&mut pts, cur, c1, c2, p);
                cur = p;
                last_ctrl = None;
            }
            b'A' => {
                let rx = sc.num();
                let ry = sc.num();
                let rot = sc.num();
                let large = sc.flag();
                let sweep = sc.flag();
                let p = [sc.num() + ox, sc.num() + oy];
                arc(&mut pts, cur, rx, ry, rot, large, sweep, p);
                cur = p;
                last_ctrl = None;
            }
            _ => break,
        }
    }
    flush(&mut pts, &mut subs, false);
    subs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_relative_moves() {
        let s = parse("M4 7h16M10 11v6");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].pts, vec![[4.0, 7.0], [20.0, 7.0]]);
        assert_eq!(s[1].pts, vec![[10.0, 11.0], [10.0, 17.0]]);
    }

    #[test]
    fn compact_arc_flags_and_a_full_circle() {
        // the rotate icon packs its flags: "a8 8 0 11-2.6-5.9"
        let s = parse("M20 12a8 8 0 11-2.6-5.9M20 4v5h-5");
        assert_eq!(s.len(), 2);
        let end = s[0].pts.last().unwrap();
        assert!((end[0] - 17.4).abs() < 0.01 && (end[1] - 6.1).abs() < 0.01, "{end:?}");
        // all points of an arc stay on the circle
        let c = parse("M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0");
        for p in &c[0].pts {
            let r = ((p[0] - 12.0).powi(2) + (p[1] - 12.0).powi(2)).sqrt();
            assert!((r - 3.0).abs() < 0.05, "{p:?} r={r}");
        }
    }

    #[test]
    fn closed_paths_report_it() {
        let s = parse("M7 5l12 7-12 7V5z");
        assert!(s[0].closed);
        assert_eq!(s[0].pts.len(), 4);
    }
}
