//! The decision half of `M.idleCheck` in js/models.js: should the model server be unloaded now?
//! The clock, the in-flight counter and the unload request belong to the caller; this is the pure rule.
use serde_json::Value;

/// State the JS keeps in closure variables.
#[derive(Debug, Clone, PartialEq)]
pub struct Idle {
    pub last_use_ms: i64,
    pub asleep: bool,
    pub inflight: u32,
}

impl Idle {
    pub fn new(now_ms: i64) -> Self {
        Idle { last_use_ms: now_ms, asleep: false, inflight: 0 }
    }

    /// A request to the model server started: it is in use and awake.
    pub fn request_started(&mut self, now_ms: i64) {
        self.last_use_ms = now_ms;
        self.asleep = false;
        self.inflight += 1;
    }

    pub fn request_finished(&mut self, now_ms: i64) {
        self.inflight = self.inflight.saturating_sub(1);
        self.last_use_ms = now_ms;
    }

    /// `idleUnloadMin` from settings: `Number(value)`; only a positive minute count enables unloading.
    pub fn minutes(setting: Option<&Value>) -> f64 {
        setting.map_or(f64::NAN, crate::jsx::number)
    }

    /// True when the caller should now unload (and the state flips to asleep, like the JS).
    pub fn check(&mut self, now_ms: i64, mins: f64, run_active: bool, queue_running: bool) -> bool {
        if !(mins > 0.0) || self.asleep || self.inflight > 0 || run_active || queue_running {
            return false;
        }
        if ((now_ms - self.last_use_ms) as f64) < mins * 60000.0 {
            return false;
        }
        self.asleep = true;
        true
    }
}
