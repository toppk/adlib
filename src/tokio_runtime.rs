//! Global Tokio runtime for async operations requiring Tokio (like hf-hub)
//!
//! GPUI uses its own async executor, but some libraries (reqwest, hf-hub)
//! require a Tokio runtime. This module provides a lazy-initialized global
//! Tokio runtime for such operations.
//!
//! Inspired by zed-industries/zed gpui_tokio crate.

use gpui::{App, Context, Task};
use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::Runtime;

static TOKIO_RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Initialize the global Tokio runtime. Call this during app startup.
pub fn init(_cx: &mut App) {
    TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime")
    });
}

/// Get the global Tokio runtime handle
pub fn handle() -> tokio::runtime::Handle {
    TOKIO_RUNTIME
        .get()
        .expect("Tokio runtime not initialized - call tokio_runtime::init() first")
        .handle()
        .clone()
}

/// Spawn a future on the Tokio runtime and return a GPUI Task
pub fn spawn<T, R, F>(cx: &mut Context<T>, future: F) -> Task<Result<R, tokio::task::JoinError>>
where
    R: Send + 'static,
    F: Future<Output = R> + Send + 'static,
{
    let handle = handle();
    let join_handle = handle.spawn(future);

    cx.foreground_executor().spawn(join_handle)
}
