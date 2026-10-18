# sciscraper-gpui

A scientific Markdown editor with an integrated RAG co-writer and reference
library, built on [GPUI](https://gpui.rs). It is the GPUI re-implementation of
[Sciscraper](https://github.com/schmettow/sciscraper-rs) and composes the
reusable components from [`ragrig-gpui`](../ragrig-gpui) with the
[`zorite-editor`](https://crates.io/crates/zorite-editor) Markdown editor.

The defining feature is the **roundtrip**: the agent reads the document, the
bibliography, and a marked passage; its answer can replace the marked passage
in place, guarded by a digest of the version you marked.

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

Useful environment variables:

| Variable | Meaning |
|---|---|
| `SCISCRAPER_WORKSPACE` | Editor/RAG workspace folder |
| `SCISCRAPER_CORPUS` | Folder indexed by the agent |
| `SCISCRAPER_BIB` | Bibliography file |
| `SCISCRAPER_FILE` | Markdown file to open at startup |
| `SCISCRAPER_ASK` | Ask this question once the window opens |
| `SCISCRAPER_SETTINGS` | Settings file (defaults to the platform config dir) |

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

- Type in the live preview. Markdown markers are styled in place; headings,
  lists, tables, math, code blocks, and block widgets are rendered by
  `zorite-editor`.
- **Preview / Raw** (toolbar, `Ctrl+Shift+E`) switches every tab between live
  preview and raw Markdown.
- Images referenced as `![](figure.png)` render inline relative to the
  document. `![](paper.pdf)`, `.epub`, `.docx`, `.html`, and `.txt` lines render
  as clickable file chips and open in the system viewer.
- Tabs show unsaved changes with `•`; `Ctrl+S` saves, `Ctrl+Shift+S` saves as,
  `Ctrl+W` closes. A dirty tab is kept until it is saved.

### Mark a passage and co-edit

1. Select a passage in the editor.
2. Press **Ask about selection** (toolbar, `Ctrl+Shift+P`). The chip
   *Marked region (N chars)* appears above the composer and the caret moves to
   the prompt.
3. Ask for a change, e.g. *"make this shorter and keep the citation"*.
4. The answer's protocol block is lifted out of the transcript. If the passage
   still matches, it is replaced in place as one undo step; the transcript
   reports the character delta. `Ctrl+Z` restores the original.
5. If the passage changed while the agent was writing, nothing is written: an
   *Insert* chip appears above the composer instead.

The same flow works if you switch tabs while the answer streams; the
replacement targets the tab the passage came from.

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
  select several to insert `[@Key1, @Key2]`, copy keys/paths, or attach the PDFs
  to the next question. Hover actions copy the key, copy the path, or open the
  PDF in the system viewer.
- The filter field matches keys, authors, years, titles, and venues.

### Co-writer

- Answers stream token by token. **Stop** keeps the partial answer; a failed
  answer can be retried.
- The conversation picker creates, loads, and refreshes saved sessions.
- The status line reports indexing, extraction, and agent errors.
- The component bar opens the settings page for the pipeline components.

### Settings

`Ctrl+,` opens the settings surface:

- **Sciscraper** — workspace, corpus folder, bibliography, live preview, pane
  visibility, and the default context attachments.
- **RAG** — provider, models, generation, prompt, retrieval, parsing/chunking,
  memory, GROBID, and the bibliography used by the worker. Press **Apply** to
  rebuild the agent.

## The roundtrip protocol

The co-writer prompt is compiled in at
`assets/prompts/sciscraper-cowriter.md`. Editor contexts use the versioned
containers the prompt documents:

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

`src/roundtrip.rs` owns the markers, container writers, char-indexed
replacement, digest, and extraction. It caps a container at 100,000 characters
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
  document.rs     EditorState per tab, save, selection, image/PDF providers
  agent.rs        ChatView + Composer + RagController, context chips, roundtrip
  library.rs      bibliography rows, corpus indexing, GROBID, Add PDFs
  app_settings.rs Sciscraper settings page
  settings.rs     persisted JSON settings and the co-writer prompt path
  roundtrip.rs    protocol, containers, digest guard, replacement extraction
  theme.rs        zorite SyntaxStyle from the GPUI Kit theme
        │
ragrig-gpui (path dependency)
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

## Checks

```sh
cargo fmt --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

Current suite: 43 tests covering the protocol, digest guard, multi-byte
ranges, replacement extraction, settings persistence, context budgeting, PDF
ingestion naming, window mounting, typing, undo/dirty tracking, preview and
settings toggling, selection capture, and guarded write-back plus undo in a
headless GPUI window.

## Known limitations

- The checked-in `Cargo.lock` is intentional: `gpui-kit 0.7.3`'s optional
  `gpui-fast` branch pins `cocoa =0.26.0` and `unicode-properties =0.1.3`,
  while a fresh resolve may pick newer patch versions. A stale lockfile fails
  with a cocoa/unicode conflict; use the checked-in one.
- Focus mode is a toolbar action; there is no floating button at the selection
  corner.
- Library rows insert on click; drag-to-cite is not implemented.
- PDFs open in the system viewer; there is no embedded PDF reader.
- `$$…$$` math and mermaid blocks are edited as source (no typesetting or
  diagram rendering); `zorite-editor` accepts host providers for both.
- There is no workspace file tree yet; files are opened with `Ctrl+O` or the
  path environment variables.
- Closing a dirty tab is refused with a status message rather than a modal
  discard confirmation.
- Images load synchronously on first paint and are cached per document; very
  large images can hitch the first frame.

## License

MIT
