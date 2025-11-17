#![forbid(unsafe_code)]

//! Core library entry point for the Rust port of the ZMK layout tooling.
//! Modules are intentionally empty placeholders until their respective
//! TDD phases are implemented (see `rust/PLAN.md`).

pub mod tokenizer;
pub mod ast;
pub mod parser;
pub mod dts;
pub mod serialization;
pub mod macro_support;
