//! Thread-safe, rate-limited progress shared by async downloads and blocking extractors.
use cocktail_shared::progress::{ProgressEvent, TransferContext};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct Progress {
    context: TransferContext,
    family: &'static str,
    state: Mutex<State>,
}
struct State {
    received: u64,
    total: Option<u64>,
    stage: Option<String>,
    last: Instant,
    terminal: bool,
}
impl Progress {
    pub fn new(family: &'static str, context: TransferContext) -> Self {
        let progress = Self {
            context,
            family,
            state: Mutex::new(State {
                received: 0,
                total: None,
                stage: None,
                last: Instant::now(),
                terminal: false,
            }),
        };
        progress.emit("started", &progress.state.lock().unwrap(), None);
        progress
    }
    fn emit(&self, suffix: &str, state: &State, error: Option<String>) {
        let event = ProgressEvent {
            transfer: self.context.clone(),
            received: state.received,
            total: state.total,
            stage: state.stage.clone(),
            error,
        };
        if let Ok(params) = serde_json::to_value(event) {
            crate::events::emit(&format!("{}.{suffix}", self.family), params);
        }
    }
    pub fn update(&self, received: u64, total: Option<u64>) {
        let mut state = self.state.lock().unwrap();
        if state.terminal {
            return;
        }
        let changed_total = state.total != total;
        state.received = received;
        state.total = total;
        if changed_total || state.last.elapsed() >= Duration::from_millis(200) {
            state.last = Instant::now();
            self.emit("progress", &state, None);
        }
    }
    pub fn add(&self, bytes: u64) {
        let mut state = self.state.lock().unwrap();
        if state.terminal {
            return;
        }
        state.received = state.received.saturating_add(bytes);
        if state.last.elapsed() >= Duration::from_millis(200) {
            state.last = Instant::now();
            self.emit("progress", &state, None);
        }
    }
    pub fn stage(&self, stage: &str) {
        let mut state = self.state.lock().unwrap();
        if state.terminal {
            return;
        }
        state.stage = Some(stage.into());
        state.last = Instant::now();
        self.emit("progress", &state, None);
    }
    pub fn finish(&self) {
        let mut state = self.state.lock().unwrap();
        if state.terminal {
            return;
        }
        state.terminal = true;
        state.total = Some(state.received);
        self.emit("completed", &state, None);
    }
    pub fn fail(&self, error: &str) {
        let mut state = self.state.lock().unwrap();
        if state.terminal {
            return;
        }
        state.terminal = true;
        self.emit("failed", &state, Some(error.into()));
    }
}

/// Counts streamed bytes without buffering a whole archive or file in memory.
pub struct ProgressReader<'a, R> {
    pub inner: R,
    pub progress: &'a Progress,
}
impl<R: std::io::Read> std::io::Read for ProgressReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.progress.add(n as u64);
        Ok(n)
    }
}
