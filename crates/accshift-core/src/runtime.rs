use std::future::Future;

/// Drive an async future to completion from synchronous code.
///
/// Builds a single-threaded tokio runtime on the current thread. Must not be
/// called from within an existing tokio runtime. Use `.await` directly there.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
        .block_on(fut)
}
