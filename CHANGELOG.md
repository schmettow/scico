# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- **Persistent edit marking and a write guard** (`gpuikit`): arming **Edit
  selection** or **Enrich selection** adds a gpuikit `RangeDecoration` (a
  selection-coloured fill and a primary frame) to the target, so the passage
  stays visibly marked after focus moves to the composer. From submit until the
  replacement is applied, the request fails, or is stopped, the target document
  is read-only; programmatic agent write-back still works, and navigation,
  selection, and copy remain available.
- **One chip for inline editing**: while an Edit or Enrich target is armed,
  Scico shows a single **Direct edit** chip and suppresses the redundant
  **Marked region** chip for the same passage. Closing the Direct edit chip
  cancels the mode, clears the range decoration, unlocks the document, drops
  the target, and collapses the native editor selection; other marked regions
  remain as context.
- **Enrich selection** (`Ctrl+Shift+R`): like **Edit selection**, but the
  marked passage is prepended to the vector retrieval query, so similarity
  search runs on the passage as well as the instruction. The passage is not
  attached twice, retrieved chunks arrive as corpus context, and the answer's
  replacement block still targets the marked region.
- **Scico prompt management**: the full prompt is compiled in from
  `assets/prompts/Scico.md` and used by default. **Settings → RAG → Chat**
  shows a prompt-file field with **Browse…**, **Open prompt**, **Export
  Scico.md**, and **Use built-in**. **Open prompt** shows the selected file in
  the editor, or the compiled prompt as an untitled `Scico.md` buffer.
  **Export Scico.md** writes the compiled prompt into the workspace, selects
  it as the active prompt, and opens it; an existing file is opened instead of
  overwritten. Saving the prompt file marks the prompt as unsaved so **Apply**
  rebuilds the agent with the new text.
- **Insert references from the library**: selecting reference rows and pressing
  **Insert** in the selection bar writes a citation into the active document
  through the roundtrip contract — `@Key` for one reference,
  `[@Key1, @Key2]` for several. The insertion replaces the editor selection or
  lands at the caret as a single undo step; clicking a row still inserts that
  one reference.
- **Fast Find clear button**: the library filter shows an X while it contains
  text, restoring the full list in one click.
- **Editor font settings**: **Settings → Editor** now offers a monospace
  family picker (installed families, filtered to monospace faces) and a font
  size control. The first launch prefers JetBrains Mono, Fira Mono, Hack, or a
  similar installed writing font and starts at 150% of the theme's mono size.
- **Ctrl+wheel zoom**: over the editor, wheel down enlarges the font and wheel
  up shrinks it; the size is persisted.
- **Block-level preview sync**: the preview follows the top-level Markdown
  block (heading, paragraph, list, table, fenced block) at the top of the
  source viewport. Blocks are mapped with comrak source positions and the
  preview is scrolled through `TextViewState`'s list, so paragraphs leave the
  preview when they leave the code window instead of drifting by pixel
  fraction. Scrolling the preview moves the source to the matching block.
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

- The system prompt is no longer an inline text field. Settings store a
  `system_prompt_path` (empty = compiled-in `Scico.md`); the file is read when
  the agent is rebuilt.
- The library inserts citations through `ragrig_gpui::Roundtrip` instead of
  calling the document's `insert_at_caret` directly, so reference insertion
  follows the same contract as agent write-back.
- The multi-select action bar moved to the top of the library list and
  merged with the selection summary: it now shows `N selected · @key1, @key2`
  and the Insert, Attach, Copy keys, Copy paths, and Clear actions in one row.
  The duplicate in-body summary is gone.
- The reference selection bar is the single place for batch actions; the
  duplicate in-body **Insert [@keys]** button was removed.
- The assistant is called **Scico** everywhere (previously “SciCo”).
- **`gpuikit` is now the default backend.** `cargo run` opens the source editor
  and rendered preview; the live-preview `zorite` backend is opt-in with
  `--no-default-features --features zorite`. The README documents both.
- Scroll sync now jumps with rendered Markdown blocks instead of mapping a pixel
  fraction; the preview is itself scrollable and exposes its top block.
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

- **Profile Defaults** no longer replaces the Scico persona with Ragrig's
  generic one-line stub; profile defaults and profiles without a prompt file
  restore the compiled-in `Scico.md`.
- Editor scrolling no longer jumps back while the mouse is over the text:
  caret-follow now listens to editor change/selection events instead of every
  repaint, and it applies one stable offset after the layout settles. It also
  cancels itself when the user scrolls during the wait.
- Typing and hovering no longer re-render the whole window: hover-only editor
  notifications update cached state silently, and caret-follow no longer
  multiplies renders with a ten-step animation.
- Session journaling: content edits are written within 300 ms, while explicit
  actions (open, save, close, tab switch, layout) still flush immediately.
- Blank icons are fixed at the root: the app registered the default
  `gpui_kit::assets::Assets` bundle, which contains only 106 of the 1,830
  icons. Icons such as Store, Chunker, Save, Sparkles, Pencil, X, Paperclip,
  and CornerDownLeft resolved to names outside that bundle. The app and demos
  now register `AllAssets`; a rasteriser regression test covers every agent-bar
  component icon.
