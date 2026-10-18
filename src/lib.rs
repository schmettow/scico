//! Sciscraper GPUI: a scientific Markdown editor with an integrated RAG
//! co-writer and reference library.
//!
//! The binary lives in `main.rs`; everything else is exposed here so the
//! roundtrip protocol and its tests can be used (and audited) independently.

pub mod agent;
pub mod app;
pub mod app_settings;
pub mod document;
pub mod library;
pub mod roundtrip;
pub mod session;
pub mod settings;
pub mod state;
pub mod theme;
