//! Resolve the editor backend before the crate is compiled.
//!
//! `gpuikit` is the default and `zorite` and `gpuikit-markdown-editor` are
//! opt-in. Cargo feature sets are additive, so `--features zorite` (without
//! `--no-default-features`) and `--all-features` enable several backends.
//! Compiling more than one backend would define several `EditorSurface`
//! implementations and several sets of editor fields, so one backend must win.
//! The block editor wins when it is enabled, then `zorite`, then `gpuikit`.
//!
//! The derived cfgs keep the source free of paired `feature = ...` checks and
//! let `cargo check --all-features` succeed instead of cascading into
//! duplicate-definition errors.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(editor_gpuikit)");
    println!("cargo:rustc-check-cfg=cfg(editor_zorite)");
    println!("cargo:rustc-check-cfg=cfg(editor_block)");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_GPUIKIT");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_ZORITE");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_GPUIKIT_MARKDOWN_EDITOR");

    let block = env::var_os("CARGO_FEATURE_GPUIKIT_MARKDOWN_EDITOR").is_some();
    let zorite = env::var_os("CARGO_FEATURE_ZORITE").is_some();
    let gpuikit = env::var_os("CARGO_FEATURE_GPUIKIT").is_some();

    if block {
        println!("cargo:rustc-cfg=editor_block");
    } else if zorite {
        println!("cargo:rustc-cfg=editor_zorite");
    } else if gpuikit {
        println!("cargo:rustc-cfg=editor_gpuikit");
    }
}
