# Roadmap

Sciscraper is a scientific Markdown editor with an integrated RAG agent and
reference management. The current prototype is functional; the API is not
stabilised.

## Now

- Keep the roundtrip experience solid across both editor backends: selection
  capture, guarded write-back, Scico edit mode, and complete crash-safe session
  restore.
- Polish the `gpuikit` writing surface: collapsible preview, synchronized
  scrolling, and theme-following syntax highlighting are in place; refine the
  reading-position mapping and large-file behaviour next.
- Keep the editor usable without the mouse: smooth scrolling, word-wise
  movement, and reliable pane toggles.

## Next

- Bring the `gpuikit` backend to functional parity: inline images and tables in
  the source surface where feasible, and PDF chips. Highlighting is already
  provided by the language-independent Syntect adapter (Markdown, BibTeX,
  LaTeX), so no Tree-sitter dependency is needed.
- Deepen reference management: richer viewer routing, corpus maintenance, and
  citation insertion workflows.
- Exercise the full pipeline against a live GROBID server and larger corpora.

## Later

- Prepare `sciscraper-gpui` and the supporting crates for publication.
- Reach functional parity with the `sciscraper-rs` prototype and retire it.
