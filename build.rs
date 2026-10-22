//! Resolve the editor backend before the crate is compiled.
//!
//! `gpuikit` is the default and `zorite` is opt-in. Cargo feature sets are
//! additive, so `--features zorite` (without `--no-default-features`) and
//! `--all-features` enable both. Compiling both backends would define two
//! `EditorSurface` implementations and two sets of editor fields, so one
//! backend must win. `zorite` wins when it is explicitly enabled; otherwise
//! `gpuikit` is used.
//!
//! The derived cfgs keep the source free of paired `feature = ...` checks and
//! let `cargo check --all-features` succeed instead of cascading into
//! duplicate-definition errors.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(editor_gpuikit)");
    println!("cargo:rustc-check-cfg=cfg(editor_zorite)");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_GPUIKIT");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_ZORITE");

    let zorite = env::var_os("CARGO_FEATURE_ZORITE").is_some();
    let gpuikit = env::var_os("CARGO_FEATURE_GPUIKIT").is_some();

    if zorite {
        println!("cargo:rustc-cfg=editor_zorite");
    } else if gpuikit {
        println!("cargo:rustc-cfg=editor_gpuikit");
    }
}
