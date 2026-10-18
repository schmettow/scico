# Roadmap

Sciscraper is a scientific Markdown editor with an integrated RAG agent and
reference management. The current prototype is functional; the API is not
stabilised.

## Now

- Keep the roundtrip experience solid: selection capture, guarded write-back,
  SciCo edit mode, and complete crash-safe session restore.
- Polish the settings surface: profiles, pending-change marks, and a resizable
  controls column.
- Keep the editor usable without the mouse: caret-follow scrolling, word-wise
  movement, and reliable pane toggles.

## Next

- Extract the Markdown editor into a reusable component with host-supplied
  block providers for file chips, math, images, and PDF embeds.
- Deepen reference management: richer viewer routing, corpus maintenance, and
  citation insertion workflows.
- Exercise the full pipeline against a live GROBID server and larger corpora.

## Later

- Prepare `sciscraper-gpui` and the supporting crates for publication.
- Reach functional parity with the `sciscraper-rs` prototype and retire it.
