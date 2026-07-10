#![no_main]

//! Fuzz the daemon IPC request decoder.
//!
//! `scotiad` reads a length-bounded frame and deserializes it into
//! [`scotia::ipc::DaemonRequest`]; that deserialization is the trust boundary
//! between an unprivileged local client and the daemon. This target feeds
//! arbitrary bytes (capped at the same 64 KiB frame limit the transport
//! enforces) through the JSON decoder and asserts only that it never panics —
//! every input must either parse cleanly or be rejected with an error.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Mirror ipc_transport::MAX_FRAME (64 KiB) so the fuzzer doesn't waste
    // budget on inputs the real transport rejects before decoding.
    if data.len() > 64 * 1024 {
        return;
    }
    let text = String::from_utf8_lossy(data);
    let _ = serde_json::from_str::<scotia::ipc::DaemonRequest>(&text);
});
