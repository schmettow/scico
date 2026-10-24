# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.5.0 — 2026-10-24

First publication on crates.io.

### Changed

- **Profiles row.** The separate **Profile** and **Profile name** fields are
  one row: choosing a profile in the picker loads it immediately, and **New**
  opens a name field with **Create** and **Cancel**. **Create** snapshots the
  current settings under the typed name and refuses duplicates; **Update
  Profile** saves the current settings to the selected profile. The separate
  **Load profile** button is gone.
- **Confirmation before closing a modified tab.** The tab's × and `Ctrl+W` now
  ask **Cancel / Don't save / Save** instead of silently refusing to close a
  dirty document. **Save** writes through the path dialog for untitled buffers
  and closes the tab only after the write succeeds; **Don't save** discards the
  buffer; **Cancel** (or `Escape`) leaves the tab untouched. The window-close
  path keeps its own Save all / Discard / Cancel dialog.
- **Workspace super container.** The left dock is now the **Workspace** — the
  project's persistence and synchronization layer. It shows the project root
  and whether `.scico/workspace.toml` is present, and its controls **New
  workspace** and **Open workspace** moved here from the toolbar. The
  **Library** is now a section and one pipeline over the workspace (corpus,
  index, and references). The toolbar toggle and the Settings page now say
  Workspace, and **Show workspace pane** replaces **Show library pane**.
- **Scico rebrand.** The application is now **Scico**. The crate and binary are
  `scico`, the workspace metadata folder is `.scico/`, environment variables
  use the `SCICO_` prefix, and the platform settings directory is `scico`.
  The assistant is now **Chat**: its prompt constants are `CHAT_PROMPT` and
  `CHAT_PROMPT_FILE`, its project prompt is `Chat.md`, and its pane labels say
  Chat. The roundtrip protocol is namespaced `scico:v1`. This is a breaking
  rename: existing workspace markers, prompt files, environment variables, and
  platform settings paths are not migrated automatically.

### Fixed

- **Saved profiles are listed after a restart.** The profile list is now read
  from `.ragrig/profiles` and `~/.ragrig/profiles` at startup and whenever a
  workspace is adopted, and the selected profile is persisted in
  `settings.json`. Previously the picker stayed empty until a profile was saved
  in the same session, so an existing profile could not be selected on the next
  launch.

### Added

- **External services settings.** A new **Settings → External services** page
  manages the GROBID server. It discovers a running instance on localhost,
  tests a URL entered by the user, or pulls and runs
  `grobid/grobid:0.9.1-full` in Docker (the full model set is the default; the
  image field is editable). The managed container binds loopback only,
  reports cancellable pull and startup progress, and is stopped when Scico
  quits. Source, URL, and image are persisted; a successful discovery or start
  updates the URL and immediately marks it for the reference pipeline.
- **Edit in BibTeX.** Every Library row has a pencil action. It opens the
  workspace bibliography (or focuses it when already open) and places the
  caret on that key's entry. The action is the shared
  `ragrig_gpui::editor` roundtrip (`ReferenceRow::on_edit`,
  `RoundtripHost::edit_reference`, `EditorSurface::reveal`), so Scico only
  implements the tab and caret handling. Saving or closing the bibliography
  bumps `AppState::references_revision`, which makes the Library reload its
  rows.
- **Workspace entry resets the view.** Creating or opening a workspace (New
  workspace, Open workspace, or a directory on the command line) closes every
  tab outside the new root, then opens the paper and the workspace
  bibliography, with the paper active. An unsaved tab outside the root is kept
  open and reported instead of being discarded.
- **Command-line workspace.** `scico <directory>` adopts an existing
  directory exactly like **Open workspace**: missing `corpus/`, `.ragrig/`,
  `.scico/`, bibliography, and `Untitled-1.md` are created, the homepage
  opens, and the choice is remembered for the next launch. The argument
  outranks `SCICO_WORKSPACE` and the persisted workspace; a file or a
  non-existent path is ignored.
- **Writer-facing README.** The README is now a guide for researchers and
  students: quick start, the edit/enrich roundtrip, library and citations,
  workspaces, shortcuts, privacy, and troubleshooting. The technical material
  moved to `DEVELOPER_NOTES.md`.
- **Chat chrome marks.** The chat component, its component bar, and the send
  button use the Chat mark; **Chat memory** sits next to the chat model. The
  welcome screen shows the full-colour square, and the assistant rows use the
  monochrome mark.
- **`UPSTREAM.md`.** Tracks requests to the upstream crates; US001 asks Ragrig
  to publish bounds/step/default with each bounded parameter.
- **Chat identity.** The README opens with the banner from `assets/logo/`.
  The empty transcript shows the square mark over “Chat knows your library.”,
  assistant rows are signed with the Chat icon instead of “Ragrig”, and the
  window receives the square as its desktop icon through the rasterised
  `Scico-square.png` (generated from `Scico square.svg` with Inkscape).
- **Workspace model and commands.** A workspace is one directory with the
  paper, `corpus/`, a `.bib`, the project `.ragrig` store/sessions/profiles,
  an optional `Chat.md`, and a `.scico/workspace.toml` descriptor
  (`bibtex`, `document`, `ragrig`, `homepage`). **New workspace**
  (`Ctrl+Shift+N`) scaffolds a new folder; **Open workspace**
  (`Ctrl+Shift+O`) adds missing pieces to an existing directory. Both report
  what was created and end with “Add PDFs to the library and chat about them.”
  The Chat header repeats the list so it stays readable.
- **Layered Ragrig configuration.** `~/.ragrig/config.json` (or
  `RAGRIG_HOME/config.json`) and `<workspace>/.ragrig/config.json` are merged
  per setting over the built-in defaults, workspace last. Profiles are listed
  and resolved from both locations, with workspace profiles shadowing global
  ones. The vector store moves to `<workspace>/.ragrig/.ragrig_store`, with
  an automatic migration from the old root-level `.ragrig_store`.
- **Status-bar switches for Chat.** The **Chat memory** and **Hybrid
  retrieval** components in the Chat bar now carry an **Enabled** switch in
  their menu. Memory off selects `MemoryChoice::Off` and restores the previous
  strategy when switched back on. Hybrid retrieval off rebuilds the worker
  with `EmbedderSpec::None`: no embedding model is required and vector search
  is skipped, while document attachments and the bibliographic library still
  reach the prompt. Both choices are persisted in `settings.json`, mirrored by
  the component dots, and also available in **Settings → RAG → Retrieval**.
- **Persistent edit marking and a write guard** (`gpuikit`): arming **Edit
  selection** or **Enrich selection** adds a gpuikit `RangeDecoration` (a
  selection-coloured fill and a primary frame) to the target, so the passage
  stays visibly marked after focus moves to the composer. From submit until the
  replacement is applied, the request fails, or is stopped, the target document
  is read-only; programmatic agent write-back still works, and navigation,
  selection, and copy remain available.
- **One chip for inline editing**: while an Edit or Enrich target is armed,
  Chat shows a single **Direct edit** chip and suppresses the redundant
  **Marked region** chip for the same passage. Closing the Direct edit chip
  cancels the mode, clears the range decoration, unlocks the document, drops
  the target, and collapses the native editor selection; other marked regions
  remain as context.
- **Enrich selection** (`Ctrl+Shift+R`): like **Edit selection**, but the
  marked passage is prepended to the vector retrieval query, so similarity
  search runs on the passage as well as the instruction. The passage is not
  attached twice, retrieved chunks arrive as corpus context, and the answer's
  replacement block still targets the marked region.
- **Chat prompt management**: the full prompt is compiled in from
  `assets/prompts/Chat.md` and used by default. **Settings → RAG → Chat**
  shows a prompt-file field with **Browse…**, **Open prompt**, **Export
  Chat.md**, and **Use built-in**. **Open prompt** shows the selected file in
  the editor, or the compiled prompt as an untitled `Chat.md` buffer.
  **Export Chat.md** writes the compiled prompt into the workspace, selects
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
  counts, and a single action row with **Apply**, **Profile Defaults**, and
  **Update Profile**. Apply is disabled while nothing is pending and violet
  when a rebuild is due.
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
  `system_prompt_path` (empty = compiled-in `Chat.md`); the file is read when
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
- The assistant is called **Chat** everywhere.
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

- A malformed workspace bibliography now reports
  `cannot parse '<path>' at line 5 (@Broken): expected =` in the Library status
  instead of a raw byte span. The loader locates the entry with the shared
  `editor::bibtex_entry_location`; a file that cannot be read names the
  reference being looked up.
- Zorite builds again with the default feature set. Cargo features are
  additive, so `--features zorite` and `--all-features` used to compile both
  editor backends and collide in `EditorSurface` and the editor state
  (33 cascading errors). `build.rs` now resolves one backend: zorite when it
  is enabled, gpuikit otherwise. Both were already aligned with the
  `ragrig-gpui` roundtrip traits; the failure was feature coexistence.
- The welcome square keeps its colours. GPUI paints `svg()` elements as a
  monochrome alpha mask, so the mark is now rendered from the rasterised PNG
  through `img` instead of the SVG. `Scico icon.svg` was redrawn as a
  monochrome target with transparent gaps for button use; the original
  multi-colour version is kept as `Scico icon colour.svg`.
- Decimal settings such as the similarity threshold accept complete drafts.
  Typing `0.03` is committed on Enter or blur; an invalid value restores the
  previous value and explains the accepted range under the field.
- Chat-memory and hybrid-retrieval switches stay inside their agent-bar
  popover: the label column may shrink, the switch keeps its width, and the
  duplicated content padding is gone.
- **Two-way preview scrolling**: wheel input over the rendered preview and
  scrollbar-thumb drags now drive the source editor, and the 250 ms feedback
  timer that snapped the preview back to an older block is gone. The editor is
  aligned to the preview's top source line with a bracketing search driven by
  its own layouts, so soft-wrapped paragraphs land on the same block; the
  resulting programmatic editor offset is consumed once and never re-scrolls
  the preview. Regression tests cover the wheel path without a jump back and
  soft-wrapped paragraphs mapping to the same block.
- **Profile Defaults** no longer replaces the Chat persona with Ragrig's
  generic one-line stub; profile defaults and profiles without a prompt file
  restore the compiled-in `Chat.md`.
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

### Quality

- `cargo llvm-cov --all-features`: 66.88% regions, 58.02% functions, 68.51%
  lines (14,395 regions, 1,072 functions, 8,240 lines). Because `build.rs`
  selects one editor backend, `--all-features` measures the zorite backend;
  the default gpuikit configuration measures 71.19% regions, 63.64%
  functions, and 72.37% lines (16,338 regions, 1,224 functions, 9,358
  lines). Doctests are not measured on stable Rust. `app.rs`, `state.rs`,
  `settings.rs`, `workspace.rs`, and `workspace_pane.rs` are the
  best-covered modules; the async worker in `agent.rs` and the library
  ingestion in `library.rs` carry most of the remainder.
- `cargo test --all-features`: 65 passed, 0 failed, 0 ignored (64 library
  tests and one integration test, zorite backend). The default gpuikit
  build runs 77 tests (76 library tests and one integration test), also
  green.
- `cargo fmt --check`, `cargo check --all-targets --all-features`,
  `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo doc --no-deps --all-features`, and `cargo package --list` are green
  (34 files).
- `cargo publish --dry-run` currently stops at dependency resolution because
  `ragrig-gpui 0.5.0` is not on crates.io yet. Publish that crate first, then
  rerun the dry run for `scico`.
- The declared MSRV is 1.97.1, matching `ragrig-gpui 0.5.0` and `ragrig` 1.2.
