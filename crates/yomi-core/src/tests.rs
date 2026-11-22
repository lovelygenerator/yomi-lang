use super::*;

#[test]
fn version_is_not_empty() {
    assert!(!version().is_empty());
}

/// Runs `f` on a thread with a 1 MiB stack, to confirm that deep nesting does not crash even in debug builds. Where threads
/// cannot be made (wasm), it runs on the current thread, whose stack is about as large.
pub(crate) fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    if cfg!(target_family = "wasm") {
        return f();
    }
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}
