//! Sciscraper GPUI: a scientific Markdown editor with an integrated RAG
//! SciCo and reference library.
//!
//! The binary lives in `main.rs`; everything else is exposed here so the
//! editor roundtrip and its tests can be used (and audited) independently.
//! The protocol itself lives in `ragrig-gpui::editor`; SciScraper implements
//! its editor contract for the zorite-backed document.

pub mod agent;
pub mod app;
pub mod app_settings;
pub mod document;
pub mod library;
pub mod session;
pub mod settings;
pub mod state;
pub mod theme;
