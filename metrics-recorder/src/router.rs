use metrics::{
    Counter, Gauge, Histogram, Key, KeyName, Recorder, SetRecorderError, SharedString, Unit,
};

use std::{
    cell::RefCell,
    fmt, mem, ptr,
    sync::{PoisonError, RwLock},
};

thread_local! {
    static LOCAL_RECORDER: RefCell<Option<Box<dyn Recorder>>> = RefCell::default();
}

static ROUTER: RecorderRouter = RecorderRouter::new();

/// Router of metric `Recorder`s that works on per-thread and global levels, like tracing
/// subscribers.
pub struct RecorderRouter {
    global: RwLock<Option<Box<dyn Recorder + Send + Sync>>>,
}

impl fmt::Debug for RecorderRouter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecorderRouter")
            .finish_non_exhaustive()
    }
}

impl RecorderRouter {
    const fn new() -> Self {
        Self {
            global: RwLock::new(None),
        }
    }

    pub fn install() -> Result<(), SetRecorderError> {
        metrics::set_recorder(&ROUTER).or_else(|err| {
            let recorder_data_ptr = ptr::addr_of!(ROUTER).cast::<()>();
            let installed_data_ptr = (metrics::recorder() as *const dyn Recorder).cast::<()>();
            if recorder_data_ptr == installed_data_ptr {
                Ok(())
            } else {
                Err(err)
            }
        })
    }

    fn with_current_recorder<T>(&self, action: impl FnOnce(&dyn Recorder) -> T) -> T {
        LOCAL_RECORDER.with(|cell| {
            let borrowed = cell.borrow();
            let lock;
            let recorder = if let Some(recorder) = borrowed.as_deref() {
                recorder
            } else {
                lock = self.global.read().unwrap_or_else(PoisonError::into_inner);
                lock.as_deref().unwrap_or(&metrics::NoopRecorder)
            };
            action(recorder)
        })
    }

    pub fn set<R: Recorder + 'static>(recorder: R) -> RecorderGuard {
        let prev_recorder = LOCAL_RECORDER.with(|cell| {
            let mut borrowed = cell.borrow_mut();
            mem::replace(&mut *borrowed, Some(Box::new(recorder)))
        });
        RecorderGuard(prev_recorder)
    }

    pub fn set_global<R>(recorder: R) -> GlobalRecorderGuard
    where
        R: Recorder + Send + Sync + 'static,
    {
        let mut lock = ROUTER
            .global
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let prev_recorder = mem::replace(&mut *lock, Some(Box::new(recorder)));
        GlobalRecorderGuard(prev_recorder)
    }
}

impl Recorder for RecorderRouter {
    fn describe_counter(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.with_current_recorder(|recorder| {
            recorder.describe_counter(key, unit, description);
        });
    }

    fn describe_gauge(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.with_current_recorder(|recorder| {
            recorder.describe_gauge(key, unit, description);
        });
    }

    fn describe_histogram(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.with_current_recorder(|recorder| {
            recorder.describe_histogram(key, unit, description);
        });
    }

    fn register_counter(&self, key: &Key) -> Counter {
        self.with_current_recorder(|recorder| recorder.register_counter(key))
    }

    fn register_gauge(&self, key: &Key) -> Gauge {
        self.with_current_recorder(|recorder| recorder.register_gauge(key))
    }

    fn register_histogram(&self, key: &Key) -> Histogram {
        self.with_current_recorder(|recorder| recorder.register_histogram(key))
    }
}

/// FIXME
#[must_use = "The recorder is reset when the guard is dropped"]
pub struct RecorderGuard(Option<Box<dyn Recorder>>);

impl fmt::Debug for RecorderGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("RecorderGuard")
            .field(&self.0.as_ref().map(drop))
            .finish()
    }
}

impl Drop for RecorderGuard {
    fn drop(&mut self) {
        let recorder = self.0.take();
        LOCAL_RECORDER.with(|cell| {
            *cell.borrow_mut() = recorder;
        });
    }
}

/// FIXME
#[must_use = "The recorder is reset when the guard is dropped"]
pub struct GlobalRecorderGuard(Option<Box<dyn Recorder + Send + Sync>>);

impl fmt::Debug for GlobalRecorderGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("GlobalRecorderGuard")
            .field(&self.0.as_ref().map(drop))
            .finish()
    }
}

impl Drop for GlobalRecorderGuard {
    fn drop(&mut self) {
        let recorder = self.0.take();
        let mut lock = ROUTER
            .global
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        *lock = recorder;
    }
}

#[cfg(test)]
mod tests {
    use metrics::Key;
    use metrics_util::{
        debugging::{DebugValue, DebuggingRecorder, Snapshotter},
        CompositeKey, MetricKind,
    };

    use std::thread;

    use super::*;

    // Not a `const` because of interior mutability of `Key`
    static COUNTER_KEY: CompositeKey =
        CompositeKey::new(MetricKind::Counter, Key::from_static_name("test"));

    fn assert_counter_value(snapshotter: &Snapshotter, expected: u64) {
        let snapshot = snapshotter.snapshot().into_vec();
        let (.., counter_value) = snapshot
            .iter()
            .find(|(key, ..)| *key == COUNTER_KEY)
            .unwrap();
        assert_eq!(*counter_value, DebugValue::Counter(expected));
    }

    #[test]
    fn router_works_as_expected() {
        RecorderRouter::install().unwrap();
        let global_recorder = DebuggingRecorder::new();
        let global = global_recorder.snapshotter();
        let global_guard = RecorderRouter::set_global(global_recorder);

        metrics::counter!("test", 2);
        assert_counter_value(&global, 2);

        // Install an additional local recorder. It should override the global recorder.
        let local_recorder = DebuggingRecorder::new();
        let local = local_recorder.snapshotter();
        let local_guard = RecorderRouter::set(local_recorder);

        metrics::counter!("test", 1);
        assert_counter_value(&local, 1);
        assert_counter_value(&global, 2);

        // Test local recorder redeclaration.
        {
            let new_local_recorder = DebuggingRecorder::new();
            let new_local = new_local_recorder.snapshotter();
            let _guard = RecorderRouter::set(new_local_recorder);

            metrics::counter!("test", 42);
            assert_counter_value(&local, 1);
            assert_counter_value(&global, 2);
            assert_counter_value(&new_local, 42);
        }

        thread::scope(|scope| {
            scope.spawn(|| {
                // The globally set recorder should still be active.
                metrics::counter!("test", 2);
                assert_counter_value(&global, 4);

                let new_local_recorder = DebuggingRecorder::new();
                let new_local = new_local_recorder.snapshotter();
                let _guard = RecorderRouter::set(new_local_recorder);

                metrics::counter!("test", 42);
                assert_counter_value(&local, 1);
                assert_counter_value(&global, 4);
                assert_counter_value(&new_local, 42);
            });
        });

        metrics::counter!("test", 2);
        assert_counter_value(&global, 4);
        assert_counter_value(&local, 3);
        drop((local_guard, local));

        metrics::counter!("test", 3);
        assert_counter_value(&global, 7);

        drop(global_guard);
        metrics::counter!("test", 3);
        assert_counter_value(&global, 7);
    }
}
