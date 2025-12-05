#![forbid(unsafe_code)]

//! Core library for ZMK keyboard layout manipulation.
//!
//! This crate provides the foundational types and functionality for parsing,
//! transforming, and serializing ZMK keyboard layouts without any Lua dependency.

pub mod adapters;
pub mod ast;
pub mod bindings;
pub mod build;
pub mod dts;
pub mod flash;
pub mod formatting;
pub mod io;
pub mod key_positions;
pub mod keymap;
pub mod layout_engine;
pub mod layout_handle;
pub mod macro_support;
pub mod parser;
pub mod prelude;
#[cfg(feature = "ancpp-preprocessor")]
pub mod preprocessor;
pub mod profiles;
pub mod providers;
pub mod serialization;
pub mod tasks;
pub mod tokenizer;
