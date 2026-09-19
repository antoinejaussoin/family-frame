use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Process-wide TTL slot. One per source; invalidate by calling [`Self::invalidate`].
pub struct TtlCache<T> {
    slot: Mutex<Option<(Instant, T)>>,
}

impl<T: Clone> TtlCache<T> {
    pub const fn new() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }

    pub fn get(&self, ttl: Duration, pred: impl FnOnce(&T) -> bool) -> Option<T> {
        let guard = self.slot.lock().ok()?;
        let (at, value) = guard.as_ref()?;
        if at.elapsed() < ttl && pred(value) {
            Some(value.clone())
        } else {
            None
        }
    }

    /// Date-keyed caches (Wikipedia on this day) ignore wall-clock TTL.
    pub fn get_untimed(&self, pred: impl FnOnce(&T) -> bool) -> Option<T> {
        let guard = self.slot.lock().ok()?;
        let (_, value) = guard.as_ref()?;
        pred(value).then(|| value.clone())
    }

    pub fn set(&self, value: T) {
        if let Ok(mut guard) = self.slot.lock() {
            *guard = Some((Instant::now(), value));
        }
    }

    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.slot.lock() {
            *guard = None;
        }
    }
}
