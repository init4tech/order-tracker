use core::time::Duration;
use init4_bin_base::deps::metrics::{counter, describe_counter, describe_histogram, histogram};
use std::sync::LazyLock;

const REQUESTS: &str = "signet.tracker.requests";
const REQUEST_DURATION_SECONDS: &str = "signet.tracker.request_duration_seconds";
const REQUEST_ERRORS: &str = "signet.tracker.request_errors";

/// Force evaluation to register all metric descriptions with the exporter.
pub(crate) static DESCRIPTIONS: LazyLock<()> = LazyLock::new(|| {
    describe_counter!(
        REQUESTS,
        "Order status requests (label: result = success / not-found / error)"
    );
    describe_histogram!(REQUEST_DURATION_SECONDS, "Duration of order status requests");
    describe_counter!(
        REQUEST_ERRORS,
        "Order status request errors (label: kind = not-found / internal)"
    );
});

pub(crate) fn record_request(result: &str) {
    counter!(REQUESTS, "result" => result.to_string()).increment(1);
}

pub(crate) fn record_request_duration(elapsed: Duration) {
    histogram!(REQUEST_DURATION_SECONDS).record(elapsed.as_secs_f64());
}

pub(crate) fn record_request_error(kind: &str) {
    counter!(REQUEST_ERRORS, "kind" => kind.to_string()).increment(1);
}
