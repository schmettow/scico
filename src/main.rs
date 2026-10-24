//! Scico GPUI entry point.

#[cfg(any(editor_zorite, editor_gpuikit))]
use std::sync::Arc;

#[cfg(any(editor_zorite, editor_gpuikit))]
use gpui_kit::{AppContext as _, WindowBounds, WindowOptions, px, size};
#[cfg(any(editor_zorite, editor_gpuikit))]
use scico::{app, workspace};

/// The desktop mark rasterised from `assets/logo/Scico square.svg`.
///
/// GPUI's window icon takes RGBA pixels, so the vector master is rendered once
/// to a 256 px PNG next to it. Re-render the PNG when the SVG changes.
#[cfg(any(editor_zorite, editor_gpuikit))]
fn window_icon() -> Option<Arc<image::RgbaImage>> {
    let bytes = include_bytes!("../assets/logo/Scico-square.png");
    match image::load_from_memory(bytes) {
        Ok(image) => Some(Arc::new(image.to_rgba8())),
        Err(error) => {
            eprintln!("failed to decode the Scico window icon: {error}");
            None
        }
    }
}

#[cfg(any(editor_zorite, editor_gpuikit))]
fn main() {
    // `scico ~/my-paper` opens that directory as a workspace and
    // scaffolds anything it is missing. Other arguments are left alone.
    let initial_workspace = workspace::directory_argument(std::env::args_os());

    gpui_kit::application()
        // The complete Lucide catalog: the default `Assets` bundle omits many
        // icons the UI uses (Store, Chunker, Save, Sparkles, Pencil, X, …).
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            ragrig_gpui::init(cx);
            #[cfg(editor_zorite)]
            zorite_editor::bind_keys(cx);
            app::bind_keys(cx);

            let bounds = gpui_kit::Bounds::centered(None, size(px(1600.), px(980.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                icon: window_icon(),
                titlebar: Some(gpui_kit::TitlebarOptions {
                    title: Some("Scico — scientific Markdown".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };

            if let Err(error) = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(move |cx| app::ScicoApp::new(window, cx, initial_workspace.clone()))
            }) {
                eprintln!("failed to open the Scico window: {error:#}");
            }
        });
}

/// The library reports which backend to enable; the binary target still needs
/// a `main` so that report is the only configuration error.
#[cfg(not(any(editor_zorite, editor_gpuikit)))]
fn main() {}
