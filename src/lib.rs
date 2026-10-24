//! Scico GPUI: a scientific Markdown editor with an integrated RAG
//! chat and reference library.
//!
//! The binary lives in `main.rs`; everything else is exposed here so the
//! editor roundtrip and its tests can be used (and audited) independently.
//! The protocol itself lives in `ragrig-gpui::editor`; Scico implements
//! its editor contract for the selected editor backend.
//!
//! `gpuikit` is the default. If both `gpuikit` and `zorite` are enabled (for
//! example with `--all-features`), `build.rs` selects `zorite` so the two
//! `EditorSurface` implementations never collide.

#[cfg(not(any(editor_zorite, editor_gpuikit)))]
compile_error!("enable an editor backend: `zorite` or `gpuikit`");

#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod agent;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod app;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod app_settings;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod document;
#[cfg(editor_gpuikit)]
pub mod highlight;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod library;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod session;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod settings;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod state;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod theme;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod workspace;
#[cfg(any(editor_zorite, editor_gpuikit))]
pub mod workspace_pane;
