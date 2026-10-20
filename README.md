# sciscraper-gpui

A scientific Markdown editor with an integrated RAG assistant, Scico, and a
reference library, built on [GPUI](https://gpui.rs). It is the GPUI re-implementation of
[Sciscraper](https://github.com/schmettow/sciscraper-rs) and composes the
reusable components from [`ragrig-gpui`](../ragrig-gpui) with the
[`gpuikit`](https://crates.io/crates/gpui-kit) source editor and a rendered
Markdown preview. The optional `zorite` backend remains available for the
original live-preview experience.

The defining feature is the **roundtrip**: the agent reads the document, the
bibliography, and a marked passage; its answer can replace the marked passage
in place, guarded by a digest of the version you marked.

## Editor backends

- **`gpuikit` (default)** — GPUI Kit's source editor with a rendered Markdown
  preview beside it. The source editor owns scrolling and has virtualized
  layout; the preview renders inline images and tables. It is built with
  `cargo run`.
- **`zorite`** — live-preview Markdown editor, including inline images, tables,
  math, and clickable PDF/EPUB/HTML chips. Build with
  `cargo run --no-default-features --features zorite`. The two features are
  mutually exclusive.

Syntax highlighting is language-independent: a Syntect-backed adapter maps
TextMate scopes onto the semantic colours of the component theme, so the
palette follows the application. Markdown, BibTeX, and LaTeX grammars ship
with Syntect; documents choose their grammar from the file extension. The
adapter parses incrementally, so an edit re-parses the changed line and reuses
the unchanged tail.

Status: **working prototype**. The editor, RAG chat, library, reference
management, settings, and co-edit roundtrip are implemented. The public API is
not stabilised.

## Requirements

- Rust 1.88 or newer.
- On Linux, GPUI links `libxkbcommon` and `libxkbcommon-x11`; the runtime
  libraries alone are not sufficient:

  ```sh
  sudo apt install libxkbcommon-dev libxkbcommon-x11-dev
  ```

- A chat backend:
  - **Ollama** (default): start `ollama serve` and pull the configured chat and
    embedding models.
  - **DeepSeek**: export `DEEPSEEK_API_KEY`; the key is read from the
    environment and never persisted.
- Optional: a running **GROBID** server on `http://localhost:8070` for parsing
  PDFs into the bibliography:

  ```sh
  docker run --rm --init --ulimit core=0 -p 8070:8070 grobid/grobid:0.9.1-crf
  ```

## Run

```sh
cargo run
```

This starts the default `gpuikit` backend. For the zorite live-preview editor:

```sh
cargo run --no-default-features --features zorite
```

Useful environment variables:

| Variable | Meaning |
|---|---|
| `SCISCRAPER_WORKSPACE` | Editor/RAG workspace folder |
| `SCISCRAPER_CORPUS` | Folder indexed by the agent |
| `SCISCRAPER_BIB` | Bibliography file |
| `SCISCRAPER_FILE` | Markdown file to open at startup |
| `SCISCRAPER_ASK` | Ask this question once the window opens |
| `SCISCRAPER_SETTINGS` | Settings file (defaults to the platform config dir) |
| `SCISCRAPER_SESSION` | Session journal (defaults to `session.json` beside the settings file) |

For a first roundtrip without a corpus:

```sh
DEEPSEEK_API_KEY=… \
SCISCRAPER_FILE=paper.md \
SCISCRAPER_WORKSPACE=$PWD \
cargo run
```

Chat still works with an empty index; Ragrig then answers as a plain chat
model.

## What to try

### Editor

- Type in the source editor. Markdown (including Quarto `.qmd`), BibTeX, and
  LaTeX files are highlighted by the grammar that matches their extension.
- **Settings → Editor** chooses the editor font family and size. The first
  launch picks an installed writing-oriented monospace family (JetBrains Mono,
  Fira Mono, Hack, and similar, in that order) at 150% of the theme's mono
  size. `Ctrl`+wheel over the editor zooms: wheel down increases the size,
  wheel up decreases it.
- **Preview** (toolbar, `Ctrl+Shift+E`) shows or hides the rendered preview in
  the `gpuikit` backend. The preview header has its own collapse button and a
  **scroll-sync** toggle. When sync is on, the preview follows the Markdown
  block at the top of the source viewport; a paragraph or heading leaves the
  preview when it leaves the code window, rather than the two panes drifting
  by pixel fraction. Scrolling the preview with the wheel or by dragging its
  scrollbar drives the source editor too: the matching source line is placed
  at the top of the code window, including through soft-wrapped paragraphs,
  and the preview stays exactly where you left it.
- The `zorite` backend keeps its **Preview / Raw** behaviour on the same
  shortcut: it switches every tab between live preview and raw Markdown.
- Images referenced as `![](figure.png)` render inline relative to the
  document. `![](paper.pdf)`, `.epub`, `.docx`, `.html`, and `.txt` lines render
  as clickable file chips and open in the configured viewer (zorite).
- Tabs show unsaved changes with `•`; `Ctrl+S` saves, `Ctrl+Shift+S` saves as,
  `Ctrl+W` closes. A dirty tab is kept until it is saved.
- `Ctrl+←` / `Ctrl+→` move by word; add `Shift` to select word by word.
- `PageUp` / `PageDown` move by a viewport; add `Shift` to extend the
  selection.
- **Library** (top left) and **Scico** (top right) toggle their panes. Both
  buttons stay in the toolbar while the pane is hidden. Drag either divider to
  resize the pane; the width is remembered.
- Closing the window with unsaved buffers asks **Save all / Discard / Cancel**.
  The session journal keeps the exact buffer text, so even a crash or a forced
  shutdown reopens every tab, the active tab, caret, pane layout, and the
  preview/scroll-sync choices. Content edits are journaled within 300 ms;
  explicit actions such as open, save, close, and layout changes flush
  immediately.

### Mark passages and co-edit

1. Select a passage in the editor and press **Ask about selection**
   (`Ctrl+Shift+P`). Repeat to attach several passages from the same or
   different tabs; each appears as its own chip above the composer. Duplicate
   ranges are ignored.
2. Ask a question. All marked passages ride along as context; a replacement
   block in the answer is offered as **Insert**, never written silently.
3. For a direct edit, select a passage and press **Edit selection**
   (`Ctrl+Shift+D`). The next prompt is treated as an edit instruction: the
   answer's protocol block replaces the marked passage in place as one undo
   step, and the transcript reports the character delta. `Ctrl+Z` restores the
   original. If more than one passage is marked, the one captured last is the
   edit target and the others remain context.
4. For an evidence-enriched edit, press **Enrich selection** (`Ctrl+Shift+R`).
   It works like **Edit selection**, but the marked passage is prepended to the
   retrieval query: the similarity search finds literature about the passage
   itself, not just about the instruction. The passage is excluded from the
   ordinary selection attachment (it is already in the query), the retrieved
   chunks still arrive as corpus context, and the answer replaces the passage.
5. Both edit modes keep the target visible: arming them adds a persistent
   selection fill and primary frame to the passage, so it stays marked after
   focus moves to the composer. The target appears above the composer as a
   single **Direct edit** chip; its **Marked region** chip is suppressed because
   it would repeat the same passage. Closing the **Direct edit** chip cancels
   the mode, removes the editor marker, collapses the native selection, and
   drops the target (other marked regions stay). On submit (`gpuikit`), the
   target document becomes read-only; user edits are rejected until the
   replacement is applied or the request fails or is stopped. The agent's own
   write-back bypasses the lock (`Document::replace` is programmatic), and
   navigation, selection, and copy still work. The `zorite` backend has no
   range decoration or read-only mode and only arms the mode.
6. If the passage changed while Scico was writing, nothing is written: an
   **Insert** chip appears above the composer instead.

The same flow works if you switch tabs while the answer streams; the edit
target stays bound to the tab the passage came from.

### Document and library context

- The **Untitled-1 / paper.md** chip above the composer attaches the active
  buffer to every question (on by default). It carries the file name, path,
  caret position, full length, and text.
- The **references.bib** chip attaches the whole bibliography, so the agent can
  turn literal citations into `@Key` citations. It is off by default.
- Both toggles are remembered across restarts.
- Mention `@Kano2025` in a question: the paper is parsed from the bibliography
  or the corpus and attached as one-shot context. Up to three papers per
  question; misses are reported in the transcript.

### Library and references

- **Add PDFs** copies PDFs into the corpus, runs GROBID, merges the records
  into the bibliography, and then indexes the corpus. Originals stay where they
  are; duplicate names get a suffix.
- **GROBID** parses the corpus folder without indexing. **Index** runs an
  incremental index update; it turns into **Stop** while running.
- Click a reference row to insert `@key` at the cursor. `+` selects a row;
  select several and use the action bar at the top of the list to **Insert**
  `[@Key1, @Key2]`, **Attach** the PDFs to the next question, **Clear** the
  selection, or copy keys and paths. The bar merges the selection summary
  (`N selected · @key1, @key2`) with the actions. Insert goes through the
  roundtrip editor contract, so it lands at the caret or over the selection as
  one undo step.
  The copy-key, copy-path, and open actions stay visible on every row and
  become fully opaque on hover.
- The open action uses the viewer assigned in **Settings → Library → External
  viewers** (PDF, EPUB, and HTML), or the platform default when the field is
  empty.
- The filter field matches keys, authors, years, titles, and venues; its **X**
  button clears the filter.

### Scico

- Answers stream token by token. **Stop** keeps the partial answer; a failed
  answer can be retried.
- The conversation picker creates, loads, and refreshes saved sessions.
- The status line reports indexing, extraction, and agent errors.
- The component bar opens the settings page for the pipeline components.

### Settings

`Ctrl+,` opens the settings surface. The divider on the right adjusts the width
of the controls column and the chosen width is remembered.

- **Overview** — profile picker and name, a per-section count of pending
  changes, and the action row: **Apply**, **Profile Defaults**, **Update
  Profile**, and **Load profile**. Apply is disabled (grey) while nothing is
  pending and violet when a rebuild is due. Profile Defaults re-reads the
  selected profile, or the built-in defaults when none exists; Update Profile
  writes the current values to the name above (creating the profile when the
  name is new); Load profile reads the selected profile from disk.
- **Library** — workspace, corpus folder, bibliography, pane visibility,
  external viewers for PDF/EPUB/HTML, the default library attachment, and
  reference ingestion (GROBID, renaming, OpenAlex, `@key` citations). Every path
  field has a **Browse…** button.
- **Editor** — rendered preview visibility and scroll sync (gpuikit), or live
  preview (zorite); editor font family and size; Scico visibility; and the
  default document attachment.
- **RAG** — provider, models, generation, prompt, retrieval, parsing/chunking,
  and memory. The token budgets are sliders over powers of two (512, 1024,
  2048, …).
- **Scico prompt** (RAG → Chat) — the compiled-in `Scico.md` is the default; an
  empty path uses it. **Browse…** selects a Markdown file that overrides it,
  **Open prompt** shows the effective prompt in the editor (a file, or an
  untitled `Scico.md` buffer for the built-in), **Export Scico.md** writes the
  built-in into the workspace, selects it, and opens it, and **Use built-in**
  drops the override. An existing `Scico.md` is never overwritten; exporting
  opens it instead. Press **Apply** after editing the prompt file; saving the
  file in the editor marks the change immediately.

RAG fields that differ from the running configuration are bold violet. Their
section and page carry a `●` in the sidebar, and the section shows an “Unsaved
changes” line; the Overview page lists how many fields changed per section.

## The roundtrip protocol

The Scico prompt is compiled in from `assets/prompts/Scico.md`. A workspace
file selected in **Settings → RAG → Chat** overrides it. Editor contexts use
the versioned containers the prompt documents:

```markdown
<sciscraper:v1:context source="creator">
<sciscraper:v1:document name="paper.md" path="paper.md" cursor="42:7" chars="12345">…</sciscraper:v1:document>
<sciscraper:v1:library name="references.bib" path="…" chars="4210">…</sciscraper:v1:library>
<sciscraper:v1:selection name="paper.md" range="120..480" chars="360" digest="b3:…">…</sciscraper:v1:selection>
</sciscraper:v1:context>
```

The result side is one fenced block:

````markdown
```sciscraper:v1:replace target="120..480"
…the finished passage…
```
````

`ragrig-gpui/src/editor/protocol.rs` owns the markers, container writers,
char-indexed replacement, digest, and extraction. It caps a container at 100,000 characters
and shortens further when the configured context window is small, marking the
excerpt with `<sciscraper:v1:truncated shown="…" total="…"/>`.

A replacement is applied only when the digest of the passage at the marked
range still matches. The model's `target` attribute is informational; the
author's mark decides where the edit lands.

## Architecture

```text
sciscraper-gpui
  app.rs          toolbar, tabs, layout, actions, file dialogs
  state.rs        documents, workspace, settings, replacement write-back
  document.rs     EditorState and TextViewState per tab, save, selection, images
  highlight.rs    incremental Syntect highlighter and Markdown/BibTeX grammars
  agent.rs        ChatView + Composer + RagController, context chips, roundtrip
  library.rs      bibliography rows, corpus indexing, GROBID, Add PDFs
  app_settings.rs Overview, Library, and Editor settings pages
  settings.rs     persisted JSON settings, viewer programs, and the Scico prompt path
  session.rs      crash-safe journal of tabs, unsaved text, caret, and layout
  theme.rs        zorite SyntaxStyle glue
  document.rs     backend-selected editor entity + EditorSurface implementation
        │
ragrig-gpui (path dependency)
  editor/         EditorSurface + RoundtripHost + Roundtrip; containers, digest guard
  RagController → worker thread → Ragrig agent/vector store/session store
  SciscraperAttach → creator/corpus containers
```

The worker owns the Tokio runtime. No Ragrig future runs on GPUI's executor.

## Upstream changes made for this app

The following were fixed in `ragrig-gpui` during development:

- `SciscraperAttach` now passes pre-built `<sciscraper:v1:…>` containers
  through verbatim instead of wrapping them in a second `document` element.
- `RagSetup` and `RagSettings` carry an explicit bibliography path; the worker
  uses it for reference rows, GROBID merging, and `@key` resolution (default
  remains `<workspace>/.ragrig/references.bib`).
- `ChatView::set_message_text` updates a finished message in place, so the host
  can lift a replacement block out of the transcript without rebuilding it.
- `path_setting` adds a **Browse…** button to every path field, and
  `token_slider` renders the 2^x token controls used by both hosts.
- The RAG page is split into `rag_model_page` plus a reusable
  `rag_references_group`, so a host can build Library / Editor / RAG pages.
- Reference-row copy actions are always visible instead of relying on a
  hover-only reveal that could leave the icons invisible.
- `RagChanges`, `RagField`, and `RagSection` expose field-level pending edits;
  `SettingRow`, `dirty_color`, and `dirty_header` render the bold-violet marks.
  `RagSettings::reset_to_profile_defaults()` backs the Overview button.
- The app registers `gpui_kit::assets::AllAssets`. The default `Assets`
  bundle contains only 106 icons and omits many names the UI uses (Store,
  Chunker, Save, Sparkles, Pencil, X, …), which was the recurring cause of
  blank buttons. The agent bar uses `Database` for the store and `Pilcrow` for
  the chunker, and a rasteriser test now guards every component icon.
- The reference selection bar gained an **Insert** action and always shows
  labelled **Insert**, **Attach**, and **Clear** buttons with visible icon
  placeholders, so the actions do not depend on a hover reveal.
- `PromptActions` lets a host add **Open prompt** and **Export …** buttons to
  the generic prompt-file row; `RagSettings` gained `system_prompt_path`,
  `built_in_prompt`, `prompt_path()`, `effective_prompt()`, and
  `use_built_in_prompt()`.
- The editor contract now covers insertion. `EditorSurface::insert_text`
  replaces the selection or inserts at `EditorSurface::caret`, as one undo
  step; `RoundtripHost::insert` and `Roundtrip::insert` expose the same seam to
  the host; `char_index_for_line_column` converts line/column carets to the
  character indices the contract uses.

## Checks

```sh
cargo fmt --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
# zorite backend
cargo check --no-default-features --features zorite --all-targets
cargo test --no-default-features --features zorite --all-targets
cargo clippy --no-default-features --features zorite --all-targets -- -D warnings
```

Current suite: 53 unit tests plus the protocol integration test, covering the
protocol, digest guard, multi-byte ranges, replacement extraction, settings and
prompt-file persistence, external viewer routing, the atomic session journal,
context budgeting, PDF ingestion naming, window mounting, typing, Ctrl+Arrow
word movement, multiple-selection capture, edit and enrich modes, undo/dirty
tracking, preview and settings toggling, pane toggling, the unsaved-changes
close guard, crash recovery into a second window, selection capture, guarded
write-back plus undo in a headless GPUI window, persistent edit decorations
with the read-only guard, incremental Markdown/BibTeX highlighting, editor font
defaults and Ctrl-wheel zoom, and two-way block-level scroll sync that stays
put in both directions, including soft-wrapped paragraphs.

## Known limitations

- The checked-in `Cargo.lock` is intentional: `gpui-kit 0.7.3`'s optional
  `gpui-fast` branch pins `cocoa =0.26.0` and `unicode-properties =0.1.3`,
  while a fresh resolve may pick newer patch versions. A stale lockfile fails
  with a cocoa/unicode conflict; use the checked-in one.
- Focus mode is a toolbar action; there is no floating button at the selection
  corner.
- Library rows insert on click; drag-to-cite is not implemented.
- PDFs open in the configured external viewer; there is no embedded PDF reader.
- `$$…$$` math and mermaid blocks are edited as source (no typesetting or
  diagram rendering); the zorite backend accepts host providers for both.
- The `gpuikit` backend shows inline images and tables in the rendered preview,
  not as atomic objects inside the source editor. Scroll sync maps top-level
  Markdown blocks; the preview parser and comrak can disagree on exotic
  constructs, in which case the two panes can be one block apart. Aligning a
  preview jump to a distant source line costs a few frames while the offset
  search converges; it never moves the preview itself.
- The Syntect grammars are packaged with the binary; themes still come from
  the component theme rather than a separate syntax-theme picker.
- There is no workspace file tree yet; files are opened with `Ctrl+O` or the
  path environment variables.
- Closing a dirty tab is still refused with a status message; the window-close
  path has the modal Save all / Discard / Cancel dialog.
- Images load synchronously on first paint and are cached per document; very
  large images can hitch the first frame.

## License

MIT
