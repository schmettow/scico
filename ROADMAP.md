# Roadmap

Sciscraper is a scientific Markdown editor with an integrated RAG agent and
reference management. The current prototype is functional; the API is not
stabilised.

## Now

- Scico branding is in place: banner, desktop mark, welcome state, and the
  assistant mark. Next: ship packaging icons (`.desktop`, `.icns`, `.ico`).
- Keep the roundtrip experience solid across both editor backends: selection
  capture, guarded write-back, Scico edit mode, and complete crash-safe session
  restore.
- Polish the `gpuikit` writing surface: collapsible preview, two-way
  block-level synchronized scrolling, theme-following syntax highlighting, and
  persisted font family/size with Ctrl+wheel zoom are in place. Refine parser
  parity for exotic Markdown next.
- Keep the editor usable without the mouse: smooth scrolling, word-wise
  movement, and reliable pane toggles.

- Edit and Enrich targets now keep a persistent range decoration, and the
  target document is read-only while the agent writes (`gpuikit`). Next: add a
  cancel path for an armed edit and use a per-range guard if gpuikit offers one.
- The Scico prompt is file-based: the full `Scico.md` is compiled in, can be
  exported into the workspace, edited in the editor, and overridden per
  profile. Refine the prompt alongside protocol changes.
- Enrich selection now runs retrieval on the marked passage itself. Next:
  compare enriched retrieval quality against ordinary selection context and
  expose the retrieved sources beside the replacement.
- The Scico status bar switches Chat memory and hybrid retrieval from the
  component menu. Next: surface the running memory strategy and retrieval
  mode in the transcript meta so saved sessions explain how an answer was
  produced.

## Next

- Bring the `gpuikit` backend to functional parity: inline images and tables in
  the source surface where feasible, and PDF chips. Highlighting is already
  provided by the language-independent Syntect adapter (Markdown, BibTeX,
  LaTeX), so no Tree-sitter dependency is needed.
- Align the preview parser's block boundaries with comrak for every extension,
  so element sync cannot drift on unusual documents.
- Deepen reference management: richer viewer routing, corpus maintenance, and
  citation insertion workflows. Library → Editor insertion now goes through the
  roundtrip contract (`Roundtrip::insert`); next are citation-key completion and
  formatting choices confined to the selected references.
- Exercise the full pipeline against a live GROBID server and larger corpora.

## Later

- Prepare `sciscraper-gpui` and the supporting crates for publication.
- Reach functional parity with the `sciscraper-rs` prototype and retire it.
