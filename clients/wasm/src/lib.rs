//! Janet World WASM client — crate root.
//!
//! Compile with:
//!
//! ```bash
//! wasm-pack build --target web --release
//! ```
//!
//! Or for development (faster, includes debug info):
//!
//! ```bash
//! wasm-pack build --target web --dev
//! ```

// Improve WASM panic messages in the browser console.
pub use console_error_panic_hook::set_once as set_panic_hook;

pub mod bridge;
pub mod cache;
pub mod client;
pub mod events;
pub mod nats_ws;

// Re-export the primary public type so consumers can do:
//   `use janet_world_wasm::JanetWorldClient;`
pub use client::JanetWorldClient;

use wasm_bindgen::prelude::*;

/// Called automatically by the generated JS glue on `init()`.
///
/// Sets up the panic hook and initialises `console_log` so that Rust
/// `log::info!` / `log::error!` calls appear in the browser DevTools console.
#[wasm_bindgen(start)]
pub fn wasm_main() {
    set_panic_hook();
    console_log::init_with_level(log::Level::Debug).ok();
    log::info!("janet-world-wasm initialised");
}
