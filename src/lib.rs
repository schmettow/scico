//! Scico GPUI: a scientific Markdown editor with an integrated RAG
//! chat and reference library.
//!
//! The binary lives in `main.rs`; everything else is exposed here so the
//! editor roundtrip and its tests can be used (and audited) independently.
//! The protocol itself lives in `ragrig-gpui::editor`; Scico implements
//! its editor contract for the selected editor backend.
//!
//! `gpuikit` is the default. `zorite` and `gpuikit-markdown-editor` are
//! opt-in. If several are enabled (for example with `--all-features`),
//! `build.rs` selects the block editor first, then `zorite`, so the
//! `EditorSurface` implementations never collide.

#[cfg(not(any(editor_zorite, editor_gpuikit, editor_block)))]
compile_error!("enable an editor backend: `zorite`, `gpuikit`, or `gpuikit-markdown-editor`");

/// The application id shared by the Wayland surface, the X11 `WM_CLASS`, and
/// the installed `.desktop` file name.
pub const APP_ID: &str = "scico";

#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod agent;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod app;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod app_settings;
#[cfg(all(target_os = "linux", any(editor_zorite, editor_gpuikit, editor_block)))]
pub mod desktop;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod document;
#[cfg(editor_block)]
pub mod editor_block;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod env;
#[cfg(editor_gpuikit)]
pub mod highlight;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod library;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod session;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod settings;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod state;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod theme;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod workspace;
#[cfg(any(editor_zorite, editor_gpuikit, editor_block))]
pub mod workspace_pane;
