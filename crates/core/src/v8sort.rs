//! `Array.prototype.sort` as V8 runs it (TimSort, `third_party/v8/builtins/array-sort.tq`).
//!
//! `nat_compare` is not a total order for names that mix BMP and supplementary-plane digits (ICU's identical
//! prefix skip), so Rust's `sort_by` can panic and, for cyclic inputs, would order differently than the JS app.
//! This is a line-for-line port of V8's algorithm: it never panics, and gives V8's order even when the
//! comparator is inconsistent.
use std::cmp::Ordering;

const MIN_GALLOP: i64 = 7;

struct Sorter<'a> {
    a: Vec<usize>,
    cmp: &'a mut dyn FnMut(usize, usize) -> i32,
    min_gallop: i64,
    runs: Vec<(usize, usize)>, // (base, length)
}

impl Sorter<'_> {
    fn c(&mut self, x: usize, y: usize) -> i32 {
        (self.cmp)(x, y)
    }

    fn min_run_length(mut n: usize) -> usize {
        let mut r = 0;
        while n >= 64 {
            r |= n & 1;
            n >>= 1;
        }
        n + r
    }

    fn count_and_make_run(&mut self, low_arg: usize, high: usize) -> usize {
        let low = low_arg + 1;
        if low == high {
            return 1;
        }
        let mut run = 2;
        let (el, eh) = (self.a[low], self.a[low - 1]);
        let desc = self.c(el, eh) < 0;
        let mut prev = el;
        for idx in low + 1..high {
            let cur = self.a[idx];
            let o = self.c(cur, prev);
            if desc {
                if o >= 0 {
                    break;
                }
            } else if o < 0 {
                break;
            }
            prev = cur;
            run += 1;
        }
        if desc {
            self.a[low_arg..low_arg + run].reverse();
        }
        run
    }

    fn binary_insertion(&mut self, low: usize, start_arg: usize, high: usize) {
        let mut start = if low == start_arg { start_arg + 1 } else { start_arg };
        while start < high {
            let (mut l, mut r) = (low, start);
            let pivot = self.a[r];
            while l < r {
                let mid = l + ((r - l) >> 1);
                let m = self.a[mid];
                if self.c(pivot, m) < 0 {
                    r = mid;
                } else {
                    l = mid + 1;
                }
            }
            self.a.copy_within(l..start, l + 1);
            self.a[l] = pivot;
            start += 1;
        }
    }

    /// Element `i` of `self.a` (`tmp` None) or of the temp copy.
    fn at(&self, tmp: Option<&[usize]>, i: i64) -> usize {
        match tmp {
            Some(t) => t[i as usize],
            None => self.a[i as usize],
        }
    }

    fn gallop_left(&mut self, tmp: Option<&[usize]>, key: usize, base: i64, length: i64, hint: i64) -> i64 {
        let (mut last, mut off) = (0i64, 1i64);
        let o = {
            let e = self.at(tmp, base + hint);
            self.c(e, key)
        };
        if o < 0 {
            let max = length - hint;
            while off < max {
                let e = self.at(tmp, base + hint + off);
                if self.c(e, key) >= 0 {
                    break;
                }
                last = off;
                off = (off << 1) + 1;
            }
            if off > max {
                off = max;
            }
            last += hint;
            off += hint;
        } else {
            let max = hint + 1;
            while off < max {
                let e = self.at(tmp, base + hint - off);
                if self.c(e, key) < 0 {
                    break;
                }
                last = off;
                off = (off << 1) + 1;
            }
            if off > max {
                off = max;
            }
            let t = last;
            last = hint - off;
            off = hint - t;
        }
        last += 1;
        while last < off {
            let m = last + ((off - last) >> 1);
            let e = self.at(tmp, base + m);
            if self.c(e, key) < 0 {
                last = m + 1;
            } else {
                off = m;
            }
        }
        off
    }

    fn gallop_right(&mut self, tmp: Option<&[usize]>, key: usize, base: i64, length: i64, hint: i64) -> i64 {
        let (mut last, mut off) = (0i64, 1i64);
        let o = {
            let e = self.at(tmp, base + hint);
            self.c(key, e)
        };
        if o < 0 {
            let max = hint + 1;
            while off < max {
                let e = self.at(tmp, base + hint - off);
                if self.c(key, e) >= 0 {
                    break;
                }
                last = off;
                off = (off << 1) + 1;
            }
            if off > max {
                off = max;
            }
            let t = last;
            last = hint - off;
            off = hint - t;
        } else {
            let max = length - hint;
            while off < max {
                let e = self.at(tmp, base + hint + off);
                if self.c(key, e) < 0 {
                    break;
                }
                last = off;
                off = (off << 1) + 1;
            }
            if off > max {
                off = max;
            }
            last += hint;
            off += hint;
        }
        last += 1;
        while last < off {
            let m = last + ((off - last) >> 1);
            let e = self.at(tmp, base + m);
            if self.c(key, e) < 0 {
                off = m;
            } else {
                last = m + 1;
            }
        }
        off
    }

    fn merge_at(&mut self, i: usize) {
        let (mut base_a, mut len_a) = self.runs[i];
        let (base_b, mut len_b) = self.runs[i + 1];
        self.runs[i] = (base_a, len_a + len_b);
        if i + 3 == self.runs.len() {
            self.runs[i + 1] = self.runs[i + 2];
        }
        self.runs.pop();
        let key_right = self.a[base_b];
        let k = self.gallop_right(None, key_right, base_a as i64, len_a as i64, 0) as usize;
        base_a += k;
        len_a -= k;
        if len_a == 0 {
            return;
        }
        let key_left = self.a[base_a + len_a - 1];
        len_b = self.gallop_left(None, key_left, base_b as i64, len_b as i64, len_b as i64 - 1) as usize;
        if len_b == 0 {
            return;
        }
        if len_a <= len_b {
            self.merge_low(base_a as i64, len_a as i64, base_b as i64, len_b as i64);
        } else {
            self.merge_high(base_a as i64, len_a as i64, base_b as i64, len_b as i64);
        }
    }

    fn merge_low(&mut self, base_a: i64, mut len_a: i64, base_b: i64, mut len_b: i64) {
        let temp: Vec<usize> = self.a[base_a as usize..(base_a + len_a) as usize].to_vec();
        let (mut dest, mut ct, mut cb) = (base_a, 0i64, base_b);
        self.a[dest as usize] = self.a[cb as usize];
        dest += 1;
        cb += 1;
        // 0 = Succeed, 1 = CopyB
        let exit: u8 = 'outer: {
            len_b -= 1;
            if len_b == 0 {
                break 'outer 0;
            }
            if len_a == 1 {
                break 'outer 1;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let (mut wins_a, mut wins_b) = (0i64, 0i64);
                loop {
                    let (x, y) = (self.a[cb as usize], temp[ct as usize]);
                    let o = self.c(x, y);
                    if o < 0 {
                        self.a[dest as usize] = self.a[cb as usize];
                        dest += 1;
                        cb += 1;
                        wins_b += 1;
                        len_b -= 1;
                        wins_a = 0;
                        if len_b == 0 {
                            break 'outer 0;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.a[dest as usize] = temp[ct as usize];
                        dest += 1;
                        ct += 1;
                        wins_a += 1;
                        len_a -= 1;
                        wins_b = 0;
                        if len_a == 1 {
                            break 'outer 1;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP || wins_b >= MIN_GALLOP || first {
                    first = false;
                    min_gallop = (min_gallop - 1).max(1);
                    self.min_gallop = min_gallop;
                    let key = self.a[cb as usize];
                    wins_a = self.gallop_right(Some(&temp), key, ct, len_a, 0);
                    if wins_a > 0 {
                        for k in 0..wins_a {
                            self.a[(dest + k) as usize] = temp[(ct + k) as usize];
                        }
                        dest += wins_a;
                        ct += wins_a;
                        len_a -= wins_a;
                        if len_a == 1 {
                            break 'outer 1;
                        }
                        if len_a == 0 {
                            break 'outer 0;
                        }
                    }
                    self.a[dest as usize] = self.a[cb as usize];
                    dest += 1;
                    cb += 1;
                    len_b -= 1;
                    if len_b == 0 {
                        break 'outer 0;
                    }
                    let key = temp[ct as usize];
                    wins_b = self.gallop_left(None, key, cb, len_b, 0);
                    if wins_b > 0 {
                        self.a.copy_within(cb as usize..(cb + wins_b) as usize, dest as usize);
                        dest += wins_b;
                        cb += wins_b;
                        len_b -= wins_b;
                        if len_b == 0 {
                            break 'outer 0;
                        }
                    }
                    self.a[dest as usize] = temp[ct as usize];
                    dest += 1;
                    ct += 1;
                    len_a -= 1;
                    if len_a == 1 {
                        break 'outer 1;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        if exit == 0 {
            for k in 0..len_a {
                self.a[(dest + k) as usize] = temp[(ct + k) as usize];
            }
        } else {
            self.a.copy_within(cb as usize..(cb + len_b) as usize, dest as usize);
            self.a[(dest + len_b) as usize] = temp[ct as usize];
        }
    }

    fn merge_high(&mut self, base_a: i64, mut len_a: i64, base_b: i64, mut len_b: i64) {
        let temp: Vec<usize> = self.a[base_b as usize..(base_b + len_b) as usize].to_vec();
        let (mut dest, mut ct, mut ca) = (base_b + len_b - 1, len_b - 1, base_a + len_a - 1);
        self.a[dest as usize] = self.a[ca as usize];
        dest -= 1;
        ca -= 1;
        // 0 = Succeed, 1 = CopyA
        let exit: u8 = 'outer: {
            len_a -= 1;
            if len_a == 0 {
                break 'outer 0;
            }
            if len_b == 1 {
                break 'outer 1;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let (mut wins_a, mut wins_b) = (0i64, 0i64);
                loop {
                    let (x, y) = (temp[ct as usize], self.a[ca as usize]);
                    let o = self.c(x, y);
                    if o < 0 {
                        self.a[dest as usize] = self.a[ca as usize];
                        dest -= 1;
                        ca -= 1;
                        wins_a += 1;
                        len_a -= 1;
                        wins_b = 0;
                        if len_a == 0 {
                            break 'outer 0;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    } else {
                        self.a[dest as usize] = temp[ct as usize];
                        dest -= 1;
                        ct -= 1;
                        wins_b += 1;
                        len_b -= 1;
                        wins_a = 0;
                        if len_b == 1 {
                            break 'outer 1;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP || wins_b >= MIN_GALLOP || first {
                    first = false;
                    min_gallop = (min_gallop - 1).max(1);
                    self.min_gallop = min_gallop;
                    let key = temp[ct as usize];
                    let k = self.gallop_right(None, key, base_a, len_a, len_a - 1);
                    wins_a = len_a - k;
                    if wins_a > 0 {
                        dest -= wins_a;
                        ca -= wins_a;
                        self.a.copy_within((ca + 1) as usize..(ca + 1 + wins_a) as usize, (dest + 1) as usize);
                        len_a -= wins_a;
                        if len_a == 0 {
                            break 'outer 0;
                        }
                    }
                    self.a[dest as usize] = temp[ct as usize];
                    dest -= 1;
                    ct -= 1;
                    len_b -= 1;
                    if len_b == 1 {
                        break 'outer 1;
                    }
                    let key = self.a[ca as usize];
                    let k = self.gallop_left(Some(&temp), key, 0, len_b, len_b - 1);
                    wins_b = len_b - k;
                    if wins_b > 0 {
                        dest -= wins_b;
                        ct -= wins_b;
                        for j in 0..wins_b {
                            self.a[(dest + 1 + j) as usize] = temp[(ct + 1 + j) as usize];
                        }
                        len_b -= wins_b;
                        if len_b == 1 {
                            break 'outer 1;
                        }
                        if len_b == 0 {
                            break 'outer 0;
                        }
                    }
                    self.a[dest as usize] = self.a[ca as usize];
                    dest -= 1;
                    ca -= 1;
                    len_a -= 1;
                    if len_a == 0 {
                        break 'outer 0;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        if exit == 0 {
            let start = dest - (len_b - 1);
            for k in 0..len_b {
                self.a[(start + k) as usize] = temp[k as usize];
            }
        } else {
            dest -= len_a;
            ca -= len_a;
            self.a.copy_within((ca + 1) as usize..(ca + 1 + len_a) as usize, (dest + 1) as usize);
            self.a[dest as usize] = temp[ct as usize];
        }
    }

    fn invariant(&self, n: usize) -> bool {
        n < 2 || self.runs[n - 2].1 > self.runs[n - 1].1 + self.runs[n].1
    }

    fn merge_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if !self.invariant(n + 1) || !self.invariant(n) {
                if n > 0 && self.runs[n - 1].1 < self.runs[n + 1].1 {
                    n -= 1;
                }
                self.merge_at(n);
            } else if self.runs[n].1 <= self.runs[n + 1].1 {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }

    fn merge_force_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if n > 0 && self.runs[n - 1].1 < self.runs[n + 1].1 {
                n -= 1;
            }
            self.merge_at(n);
        }
    }

    fn run(&mut self) {
        let mut remaining = self.a.len();
        let mut low = 0;
        let min_run = Self::min_run_length(remaining);
        while remaining != 0 {
            let mut cur = self.count_and_make_run(low, low + remaining);
            if cur < min_run {
                let forced = min_run.min(remaining);
                self.binary_insertion(low, low + cur, low + forced);
                cur = forced;
            }
            self.runs.push((low, cur));
            self.merge_collapse();
            low += cur;
            remaining -= cur;
        }
        self.merge_force_collapse();
    }
}

/// `v.sort(cmp)` with V8's exact comparison sequence; `Less` is JS `< 0`.
pub fn sort_by<T, F: FnMut(&T, &T) -> Ordering>(v: &mut Vec<T>, mut f: F) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let order = {
        let mut cmp = |x: usize, y: usize| -> i32 {
            match f(&v[x], &v[y]) {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            }
        };
        let mut s = Sorter { a: (0..n).collect(), cmp: &mut cmp, min_gallop: MIN_GALLOP, runs: Vec::new() };
        s.run();
        s.a
    };
    let mut slots: Vec<Option<T>> = std::mem::take(v).into_iter().map(Some).collect();
    *v = order.into_iter().map(|i| slots[i].take().expect("permutation")).collect();
}
