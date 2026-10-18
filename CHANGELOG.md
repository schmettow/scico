# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- Quarto `.qmd` files open like Markdown: the file dialog accepts them, and the
  editor highlights them with the Markdown grammar.
- **Syntax highlighting** in the `gpuikit` backend. A language-independent
  Syntect adapter maps TextMate scopes onto the component theme's semantic
  colours, so the palette keeps following the application. Markdown, BibTeX,
  and LaTeX grammars are selected from the file extension; untitled buffers are
  Markdown. Parsing is incremental: an edit re-parses the changed line and
  reuses the unchanged tail.
- **Collapsible preview** (`gpuikit`). The preview has a slim header with a
  collapse button, and the toolbar's **Preview** button (or `Ctrl+Shift+E`)
  brings it back. The choice is persisted.
- **Scroll sync** (`gpuikit`). The preview header has a link toggle; with sync
  on, scrolling either the source or the preview moves the other. A short
  suppression window prevents the two directions from fighting while the
  editor applies a deferred offset. The toggle is persisted.
- **`gpuikit` editor backend** behind a mutually exclusive feature. The source
  editor owns scrolling and virtualization, and a rendered Markdown preview
  beside it keeps inline images and tables visible. Both backends implement the
  same `EditorSurface` / `RoundtripHost` contract, so the agent and roundtrip
  logic are unchanged.
- **Roundtrip contract moved to `ragrig-gpui::editor`.** `Document` implements
  `EditorSurface`, `AppStateHost` implements `RoundtripHost`, and `AgentPane`
  holds a `Roundtrip` façade. The containers, digest guard, and replacement
  extraction are now shared with other hosts; the zorite backend stays behind
  the trait.
- `PageUp` / `PageDown` (and `Shift` variants) navigate the editor by a
  viewport, using the editor’s own goal-column movement.
- **Settings → Overview**: profile picker and name, per-section pending-change
  counts, and a single action row with **Apply**, **Profile Defaults**,
  **Update Profile**, and **Load profile**. Apply is disabled while nothing is
  pending and violet when a rebuild is due.
- Field-level unsaved marks: RAG settings that differ from the running agent are
  bold violet, their sections and pages carry `●` in the sidebar, and changed
  sections show an “Unsaved changes” line.
- Adjustable settings column: drag the divider on the right; the width is
  persisted with the other settings.
- `RagSettings::reset_to_profile_defaults()` backs **Profile Defaults** (reload
  the selected profile, or the built-in defaults while keeping the workspace
  and bibliography).

### Changed

- The assistant is called **Scico** everywhere (previously “SciCo”).
- **`gpuikit` is now the default backend.** `cargo run` opens the source editor
  and rendered preview; the live-preview `zorite` backend is opt-in with
  `--no-default-features --features zorite`. The README documents both.
- The Editor settings page follows the backend: rendered preview visibility and
  scroll sync for `gpuikit`, live preview for `zorite`.
- Roundtrip write-back now carries `&mut Window`, so a replacement can be
  applied from the render path with both backends.
- The RAG page now contains only Chat, Retrieval, Parsing and chunking, and
  Memory. Profiles and Apply moved to Overview and are no longer a section in
  the page hierarchy.
- External viewer and path fields fit the resizable controls column; **Browse…**
  no longer clips at narrow widths.

### Fixed

- Editor scrolling no longer jumps back while the mouse is over the text:
  caret-follow now listens to editor change/selection events instead of every
  repaint, and it applies one stable offset after the layout settles. It also
  cancels itself when the user scrolls during the wait.
- Typing and hovering no longer re-render the whole window: hover-only editor
  notifications update cached state silently, and caret-follow no longer
  multiplies renders with a ten-step animation.
- Session journaling: content edits are written within 300 ms, while explicit
  actions (open, save, close, tab switch, layout) still flush immediately.
- The `Copy @key` action, the vector-store component, and the chunker component
  rendered blank icons. Copy now uses the `Copy` glyph; the store and chunker
  use `Layers` and `Columns3`, whose SVGs the rasteriser supports.
