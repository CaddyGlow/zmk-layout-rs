#![forbid(unsafe_code)]

//! Re-exports for backwards compatibility with zmk_layout_rs imports.

// Re-export everything from core
pub use zmk_layout_core::*;

// Re-export lua API
pub use zmk_layout_lua::lua_api;
pub use zmk_layout_lua::lua_engine;

// CLI module (local)
pub mod cli;
