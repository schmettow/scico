//! Sciscraper GPUI entry point.

use gpui_kit::{AppContext as _, WindowBounds, WindowOptions, px, size};
use sciscraper_gpui::app;

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
