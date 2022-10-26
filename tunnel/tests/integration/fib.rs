use tracing::field;

use std::{
    error, fmt,
    sync::{Arc, Mutex},
};

use tracing_tunnel::{TracingEvent, TracingEventSender};

#[derive(Debug)]
struct Overflow;

impl fmt::Display for Overflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "integer overflow")
    }
}

impl error::Error for Overflow {}

#[tracing::instrument(target = "fib", ret, err)]
fn compute(count: usize) -> Result<u64, Overflow> {
    let (mut x, mut y) = (0_u64, 1_u64);
    for i in 0..count {
        tracing::debug!(target: "fib", i, current = x, "performing iteration");
        (x, y) = (y, x.checked_add(y).ok_or(Overflow)?);
    }
    Ok(x)
}

const PHI: f64 = 1.618033988749895; // (1 + sqrt(5)) / 2

pub fn fib(count: usize) {
    let span = tracing::info_span!("fib", approx = field::Empty);
    let _entered = span.enter();

    let approx = PHI.powi(count as i32) / 5.0_f64.sqrt();
    let approx = approx.round();
    span.record("approx", approx);

    tracing::warn!(count, "count looks somewhat large");
    match compute(count) {
        Ok(result) => {
            tracing::info!(result, "computed Fibonacci number");
        }
        Err(err) => {
            tracing::error!(error = &err as &dyn error::Error, "computation failed");
        }
    }
}

/// **NB.** Must be called once per program run; otherwise, call sites will be missing
/// on subsequent runs.
pub fn record_events(count: usize) -> Vec<TracingEvent> {
    let events = Arc::new(Mutex::new(vec![]));
    let events_ = Arc::clone(&events);
    let recorder = TracingEventSender::new(move |event| {
        events_.lock().unwrap().push(event);
    });

    tracing::subscriber::with_default(recorder, || fib(count));
    Arc::try_unwrap(events).unwrap().into_inner().unwrap()
}
