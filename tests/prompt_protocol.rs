//! The compiled SciCo prompt must document every protocol marker.
//!
//! The protocol lives in `ragrig-gpui`; the prompt is Sciscraper-specific, so
//! this is the one test that deliberately spans both crates.

use ragrig_gpui::editor::missing_markers;

#[test]
fn the_prompt_knows_every_marker() {
    let prompt = include_str!("../assets/prompts/Scico.md");
    assert!(
        missing_markers(prompt).is_empty(),
        "{:?}",
        missing_markers(prompt)
    );
}
