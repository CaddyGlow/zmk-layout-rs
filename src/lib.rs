#![forbid(unsafe_code)]

//! Core library entry point for the Rust port of the ZMK layout tooling.
//! Modules are intentionally empty placeholders until their respective
//! TDD phases are implemented (see `rust/PLAN.md`).

pub mod adapters;
pub mod ast;
pub mod bindings;
pub mod build;
pub mod dts;
pub mod layout_engine;
pub mod macro_support;
pub mod parser;
pub mod providers;
pub mod serialization;
pub mod tasks;
pub mod tokenizer;
