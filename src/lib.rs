//! Sciscraper GPUI: a scientific Markdown editor with an integrated RAG
//! Scico and reference library.
//!
//! The binary lives in `main.rs`; everything else is exposed here so the
//! editor roundtrip and its tests can be used (and audited) independently.
//! The protocol itself lives in `ragrig-gpui::editor`; SciScraper implements
//! its editor contract for the selected editor backend.

#[cfg(all(feature = "zorite", feature = "gpuikit"))]
compile_error!("enable exactly one editor backend: `zorite` or `gpuikit`");

#[cfg(not(any(feature = "zorite", feature = "gpuikit")))]
compile_error!("enable an editor backend: `zorite` or `gpuikit`");

pub mod agent;
pub mod app;
pub mod app_settings;
pub mod document;
#[cfg(feature = "gpuikit")]
pub mod highlight;
pub mod library;
pub mod session;
pub mod settings;
pub mod state;
pub mod theme;
pub mod workspace;
