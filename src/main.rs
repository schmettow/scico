//! Sciscraper GPUI entry point.

use std::sync::Arc;

use gpui_kit::{AppContext as _, WindowBounds, WindowOptions, px, size};
use sciscraper_gpui::app;

/// The desktop mark rasterised from `assets/logo/Scico square.svg`.
///
/// GPUI's window icon takes RGBA pixels, so the vector master is rendered once
/// to a 256 px PNG next to it. Re-render the PNG when the SVG changes.
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

fn main() {
    gpui_kit::application()
        // The complete Lucide catalog: the default `Assets` bundle omits many
        // icons the UI uses (Store, Chunker, Save, Sparkles, Pencil, X, …).
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            ragrig_gpui::init(cx);
            #[cfg(feature = "zorite")]
            zorite_editor::bind_keys(cx);
            app::bind_keys(cx);

            let bounds = gpui_kit::Bounds::centered(None, size(px(1600.), px(980.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                icon: window_icon(),
                titlebar: Some(gpui_kit::TitlebarOptions {
                    title: Some("Sciscraper — scientific Markdown".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };

            if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| app::SciscraperApp::new(window, cx))
            }) {
                eprintln!("failed to open the Sciscraper window: {error:#}");
            }
        });
}
