# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.7.2 — 2026-11-13

### Fixed

- **Provider API keys reach GUI launches on Linux.** Desktop sessions do not
  source shell startup files, so a Scico started from the application menu was
  blind to keys exported in `~/.bashrc`; Windows was unaffected because its
  user environment is inherited by Explorer launches. Scico now reads
  `<config dir>/scico/env` at the top of `main`, before any thread starts, and
  leaves variables already present in the environment untouched. New
  `src/env.rs` with parser and precedence tests; `src/main.rs` loads it;
  `src/app_settings.rs` names the file in the Profiles footer; README and
  DEVELOPER_NOTES document the format and the desktop-launcher difference.
- **Every queued paper is named in the transcript.** A prompt used to mention
  only the active document container (`paper.md`), so papers attached from the
  Library looked absent even though the worker received them.
  `AgentPane::send_prompt` now emits one `Paper attached: @Key` note per queued
  paper, and `Context attached: …` for in-memory blocks, before the answer
  (`src/agent.rs`).
- **Settings changes no longer reset the pending context.** Toggling Chat
  memory or Hybrid retrieval rebuilds the worker and produces a session event;
  the pane treated that as a conversation switch and dropped its attachments,
  selections, and armed edit. It now consults
  `RagController::session_preserves_context()`: a rebuild keeps the queued
  context and a sent direct edit releases its lock, while an explicit New or
  Load still starts clean (`src/agent.rs`).
- **The embedding provider survives a restart.** The RagRig page now offers
  Ollama, llama.cpp, OpenAI, Mistral, and OpenRouter embeddings;
  `PersistedSettings` stores the provider label and restores it before the
  agent is built, so a cloud embedding choice is not reset to Ollama
  (`src/settings.rs`).

### Quality

Release validation on 2026-11-13 (Cargo 1.99.0, rustc 1.99.0).

**Test coverage.** `cargo llvm-cov --all-features --summary-only` (doctests are
not measured on stable Rust):

| Metric | Covered | Total | Cover |
| --- | ---: | ---: | ---: |
| Regions | 12,617 | 17,701 | 71.28% |
| Functions | 820 | 1,307 | 62.74% |
| Lines | 7,366 | 10,162 | 72.49% |

`--all-features` selects the zorite backend, so the gpuikit-only
`src/highlight.rs` is not measured in this run; `src/main.rs` is the binary
entry point and stays at 0% lines. The new attachment-note helper and the
preserved/dropped session cases in `src/agent.rs` are covered by unit and app
tests respectively, and `src/settings.rs` (94.76% lines) covers the persisted
embedding-provider round trip.

**Tests.** `cargo test --all-features`: 99 passed, 0 failed (97 lib, 1 prompt
protocol, 1 working directory; no doctests).

**Checks.** `cargo fmt --check`, `cargo check --all-targets --all-features`,
`cargo clippy --all-targets --all-features -- -D warnings`,
`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`, and `cargo
package --list --allow-dirty` all pass.

**Release blocker.** `cargo publish --dry-run --allow-dirty` fails until
`ragrig-gpui` 0.9.2 is live:

```text
failed to select a version for the requirement `ragrig-gpui = "^0.9.2"`
candidate versions found which didn't match: 0.9.1, 0.9.0, 0.7.1, ...
```

Publish `ragrig-gpui` 0.9.2 first, wait for the crates.io index to update, then
publish this crate. The `[patch.crates-io]` entries are development-only and
are stripped from the package.

## v0.7.1 — 2026-11-10

### Performance

- Typing, scrolling, and hovering no longer rebuild the whole workspace in
  `src/library.rs`, `src/state.rs`, and `src/document.rs`:
  - the reference list is virtualized, so a bibliography of any size builds only
    the visible rows;
  - the library and Chat panes ignore app-state notifications that do not change
    their content;
  - the editor observer recognizes hover/scroll repaints without copying and
    hashing the buffer;
  - Markdown block parsing for preview scroll sync is deferred until it is
    needed and is skipped entirely while the preview is hidden.
- Session journal serialization and writes run on a writer thread in
  `src/session.rs`; explicit saves and window close still wait for the file.
  This removes the 40–200 ms stall that followed a typing pause on a large
  document.
- The window title is sent to the platform only when it changes
  (`src/app.rs`), avoiding a request per frame on Wayland and X11.

### Quality

Release validation on 2026-11-10 (Cargo 1.99.0, rustc 1.99.0).

**Test coverage.** `cargo llvm-cov --all-features --summary-only` (doctests are
not measured on stable Rust):

| Metric | Covered | Total | Cover |
| --- | ---: | ---: | ---: |
| Regions | 12,207 | 17,342 | 70.39% |
| Functions | 775 | 1,265 | 61.26% |
| Lines | 7,092 | 9,919 | 71.50% |

`--all-features` selects the zorite editor (`build.rs`), so gpuikit-only files
and branches, including `src/highlight.rs`, are not measured by this run. The
least covered areas are the binary entry point (`src/main.rs`, 0%; the library
is tested), the Library pane (`src/library.rs`, 49.03%; its virtual list is
covered by the app layout tests), the agent pane (`src/agent.rs`, 42.76%), and
the settings pages (`src/app_settings.rs`, 57.43%). The settings, startup, and
workspace behavior lives in `src/settings.rs` (94.71% lines), `src/state.rs`
(68.66%), and `src/workspace.rs` (90.72%).

**Tests.** `cargo test --all-features`: 89 library tests + 2 integration tests,
**91 passed, 0 failed** (zorite backend). `cargo test` (default gpuikit
backend): 104 library tests + 2 integration tests, **106 passed, 0 failed**.
The new performance tests in the default suite cover repaint-only editor
notifications, hidden-preview parsing, library virtualization, unrelated
state notifications, and the debounced session journal.

**Checks.** `cargo fmt --check`, `cargo check --all-targets --all-features`,
`cargo test --all-features`, `cargo test`, `cargo clippy --all-targets
--all-features -- -D warnings`, and `cargo doc --no-deps --all-features` (with
`RUSTDOCFLAGS="-D warnings"`) are green. `cargo package --list --allow-dirty`
lists 36 files.

**Release blocker (expected publication order).** Both `cargo package` and
`cargo publish --dry-run` currently fail to prepare scico 0.7.1 because it
requires `ragrig-gpui = "^0.9.1"`, while crates.io offers 0.9.0. Publish
`ragrig-gpui 0.9.1` first, then re-run the scico dry run. `grobid-bibtex`
0.4.0 is already on crates.io. Scico's `[patch.crates-io]` entries are
development-only and Cargo strips them from the packaged manifest.

## v0.7.0 — 2026-11-08

### Added

- **First-run setup guide.** On first launch, when the selected workspace has
  no Ragrig or Scico data, Scico opens a guided setup. It walks through the local
  embedding model and its Ollama source (existing server, existing container,
  or managed Docker container), the chat provider and model with API-key
  guidance (frontier providers and local Ollama/llama.cpp), context and answer
  limits suggested from the selected model, retrieval top-k and similarity
  threshold, optional GROBID, and a review. **Set up later** restores the
  settings from before the guide; **Settings → Overview → Setup guide** runs it
  again. A completed or skipped guide leaves a marker in `.ragrig`.
- **Managed Ollama and llama.cpp containers.** *Settings → External services*
  now has an **Ollama** and a **llama.cpp** group next to GROBID. Each service
  can use a server that is already running, start and stop a hand-configured
  container by name (**Existing container**), or pull and run an official
  Docker image. **Auto start** starts a container when Scico opens; **Stop**,
  **Remove container**, **Cancel**, and pull/start progress work like the
  GROBID group. Managed containers publish on `127.0.0.1:11434` (Ollama) and
  `127.0.0.1:8080` (llama.cpp).
- **Existing container source.** Enter a container name such as `ollama-sycl`
  and Scico starts and stops it without creating, recreating, or removing it.
  It is left running when the app quits, so a container with custom devices,
  runtimes, users, or volumes keeps its configuration. Container names are
  stored in `settings.json` for all three services.
- **Ollama model provisioning.** **Pull models** fetches the configured chat,
  embedding, and memory models through the container's API and reports
  per-model progress. Models live in the `ragrig-ollama-models` volume and
  survive container removal.
- **Managed llama.cpp configuration.** The group accepts a Hugging Face model
  (`owner/repo[:quant]`), an absolute `.gguf` path, and extra `llama-server`
  arguments. Hugging Face downloads persist in the `ragrig-llamacpp-models`
  volume. The URL from the Chat group is updated when the managed server is
  ready.
- **Actionable Docker errors.** A stopped daemon reports how to start Docker
  Desktop or `docker.service`; a user without socket access gets the
  `usermod -aG docker` remediation. Port conflicts and platform mismatches name
  the service and the fix. A startup timeout includes the last container log
  lines.
- Service source, image, auto-start, llama.cpp model, and arguments are
  persisted in `settings.json`. Older settings files pick up the defaults.

### Changed

- Scico now requires `ragrig-gpui` 0.9 and builds against its generic
  `ExternalServices` controller. The GROBID-only `grobid` field in `AppState`
  became `services`.
- A successful managed start or adopted existing server promotes the service
  source and URL into the applied configuration immediately.

### Fixed

- **The implicit home fallback is never written as a workspace layer.** The
  memory and hybrid switches are recorded in `.ragrig/config.json` only when a
  workspace was explicitly chosen.
- **Restarting no longer resets the settings a workspace layer did not name.**
  Scico saves the **Chat memory** and **Hybrid retrieval** switches in
  `<workspace>/.ragrig/config.json`. That layer used to replace the whole RAG
  configuration with library defaults, so context size, maximum tokens,
  temperature, the prompt, and every GROBID/Ollama/llama.cpp source, image,
  and container name reverted on the next launch. The layer is now a partial
  overlay on `settings.json`, and the surviving values are written back
  unchanged.
- **Profiles keep host-owned state.** Loading a profile or pressing **Profile
  Defaults** no longer resets the bibliography, streaming, rename/OpenAlex/
  citation switches, hybrid retrieval, or external-service choices, and it no
  longer discards the models chosen for other providers. Every profile-backed
  field is covered by a round-trip test; the complete `settings.json` round
  trip now checks all providers and all three services.
- **Named folder corpora persist.** The folder corpora in *Settings → RAG →
  Retrieval* (`name=path` entries) now round-trip through `settings.json`
  instead of surviving only in the running session or a profile.
- `pull access denied` and container-runtime permission errors are no longer
  reported as “Docker running, but user cannot access it”. The message now names
  the image/tag or the container device/mount, and a locally built image is used
  without a registry pull. A hand-configured container (for example an Intel
  SYCL Ollama) can now be started directly with **Existing container**.

### Quality

Release validation on 2026-11-08 (Cargo 1.99.0, rustc 1.99.0).

**Test coverage.** `cargo llvm-cov --all-features --summary-only` (doctests are
not measured on stable Rust):

| Metric | Covered | Total | Cover |
| --- | ---: | ---: | ---: |
| Regions | 11,401 | 16,699 | 68.27% |
| Functions | 732 | 1,219 | 60.05% |
| Lines | 6,612 | 9,530 | 69.38% |

The least covered areas are the binary entry point (`src/main.rs`, 0%; the
library is tested), the Library pane (`src/library.rs`, 34.86%), the agent pane
(`src/agent.rs`, 42%), and the settings pages (`src/app_settings.rs`, 57%).
The settings, startup, and workspace behavior lives in `src/settings.rs`
(94.71% lines), `src/state.rs` (65.85%), and `src/workspace.rs` (90.72%).

**Tests.** `cargo test --all-features`: 83 library tests + 2 integration tests,
all passing. `cargo test`: 96 library tests + 2 integration tests, all passing.
Coverage includes layered settings that preserve host choices, profile
round-trips, the persisted service and corpus fields, first-launch onboarding
and its workspace marker, rendering every onboarding step for local and
frontier providers, and the structured working-directory/protocol tests.

**Checks.** `cargo fmt --check`, `cargo check --all-targets --all-features`,
`cargo test --all-features`, `cargo test`, `cargo clippy --all-targets
--all-features -- -D warnings`, and `cargo doc --no-deps --all-features` (with
`RUSTDOCFLAGS="-D warnings"`) are green. `cargo package --list --allow-dirty`
lists 36 files.

**Release blocker (expected publication order).** `cargo publish --dry-run
--allow-dirty` cannot resolve `ragrig-gpui = "^0.9.0"`: crates.io currently
offers 0.7.1. Publish `ragrig-gpui 0.9.0` first, then remove scico's
`[patch.crates-io]` section (the local `ragrig-gpui` and `grobid-bibtex`
paths), update `Cargo.lock`, and re-run the dry run. `grobid-bibtex` 0.4.0 is
already on crates.io. This is the expected two-crate order, not a code or
packaging defect.

## v0.5.3 — 2026-11-04

### Fixed

- **The toolbar follows the editor state.** Without an open document only
  **New** and **Open** remain active; **Save**, **Save as**, and **Preview**
  are greyed out. **Ask about selection**, **Edit selection**, and **Enrich
  selection** activate only when the editor has a marked passage — one
  character is enough, whitespace included. Workspace, Settings, and Chat stay
  available because they do not depend on a document.
- **The Library's BibTeX button is a toggle.** Opening the raw bibliography
  still blocks GROBID; clicking **BibTeX** again saves the tab and closes it,
  so the bibliography on disk is current and GROBID can resume. A failed save
  keeps the tab open and reports the error.
- **Open dialogs start in the workspace.** **Open file** (Ctrl+O), **Open
  workspace** (Ctrl+Shift+O), and **Import PDFs** now pass their starting
  folder to the platform chooser. On Linux this uses the XDG portal's
  `current_folder`; GPUI's default prompt has no such option. The workspace
  layout **Browse…** buttons start at the current path in each field.
- **The working directory follows the workspace.** Whenever a workspace is
  open, Scico changes the process working directory to its root; a plain
  launch without a workspace uses the home directory. Relative paths in file
  dialogs, Markdown links, and external tools therefore agree, instead of
  inheriting whatever directory the launcher happened to use.
- **The implicit workspace fallback is the home directory**, not the launch
  directory. It is still never persisted or scaffolded. A relative path passed
  to the editor is now resolved against the open workspace.
- **Workspace layout shows `~/…` paths.** The workspace, corpus, and
  bibliography fields abbreviate paths below the home directory and expand a
  typed `~` back to an absolute path. All three fields are 150% of their
  previous width (510 px instead of 340 px), and edits apply on Enter or blur
  so a half-typed `~` cannot scaffold the home directory.
- **Bibliography links are portable.** With `ragrig-gpui` 0.7, GROBID writes
  `file` fields relative to the workspace (home when no workspace is open),
  and the Library and `@key` citations read absolute, workspace-relative,
  bibliography-relative, and corpus-relative links. A PDF on another drive
  stays absolute.

### Changed

- Scico now requires `ragrig-gpui` 0.7, which builds on `grobid-bibtex` 0.4.

### Quality

- Validated with `cargo fmt --check`, `cargo check --all-targets
  --all-features`, `cargo test --all-features` (76 library tests + 2
  integration tests, all passing), `cargo test` (89 library tests + 2
  integration tests, all passing), `cargo clippy --all-targets --all-features
  -- -D warnings` and `cargo doc --no-deps --all-features` (all green).

## v0.5.2 — 2026-11-01

### Added

- **llama.cpp chat provider.** Scico now builds against `ragrig-gpui` 0.6.1,
  so *Settings → RAG → Chat → Provider* offers the local **llama.cpp** option
  and its **llama.cpp URL** row. The URL is persisted in `settings.json`, so a
  server on a non-default port survives restarts. Previously the crate stayed
  on the published `ragrig-gpui` 0.5.x and Cargo ignored the local patch.
- **Save as and real names for new papers.** The toolbar now has a **Save as**
  action next to **Save**. Saving an `Untitled-x.md` placeholder asks for a real
  file name, removes the empty placeholder, and updates the workspace
  descriptor so the renamed paper becomes the homepage.
- **GNOME and other client-decorated Wayland sessions get a title bar.** When
  the compositor does not draw server-side decorations, Scico now renders its
  own title bar with the document title, drag-to-move, double-click maximize,
  and minimize/maximize/close controls. KDE, X11, Windows, and macOS keep the
  native frame, so no second bar appears there.
- **Linux launchers recognize Scico.** On first launch Scico installs
  `scico.desktop` and its icons below `XDG_DATA_HOME` and sets the Wayland
  `app_id` / X11 `WM_CLASS` to `scico`. The GNOME dash, the app switcher, and
  the X11 task bar show the Scico mark and group the window under the app
  instead of a generic executable.

### Quality

Release validation on 2026-11-01 (Cargo 1.99.0, rustc 1.99.0).

**Test coverage.** `cargo llvm-cov --all-features --summary-only` (doctests are
not measured on stable Rust):

| Metric | Covered | Total | Cover |
| --- | ---: | ---: | ---: |
| Regions | 10,365 | 15,312 | 67.69% |
| Functions | 669 | 1,135 | 58.94% |
| Lines | 6,021 | 8,719 | 69.06% |

The least covered files are the binary entry point (`src/main.rs`, 0%), the
RAG worker (`src/agent.rs` 42.02%), the library pane (`src/library.rs`
48.31%), the persisted-settings surface (`src/app_settings.rs` 56.67%), and
the application state (`src/state.rs` 65.65%).  The window-driving entry point
is exercised by running the application rather than by headless tests.

`--all-features` selects the zorite editor in `build.rs`, so the gpuikit-only
test set does not run under that configuration.  The default-feature run is
included below to cover it.

**Tests.** `cargo test --all-features`: 74 library tests and 1 integration
test — **75 passed, 0 failed, 0 ignored**.  `cargo test` (default `gpuikit`
feature): 87 library tests and 1 integration test — **88 passed, 0 failed, 0
ignored**.  There are no doctests.

**Checks.** `cargo fmt --check`, `cargo check --all-targets --all-features`,
`cargo test --all-features`,
`cargo clippy --all-targets --all-features -- -D warnings`, and
`cargo doc --no-deps --all-features` are green.  `cargo package --list --allow-dirty`
lists 35 files.

**Release blocker.** `cargo publish --dry-run --allow-dirty` cannot verify
scico 0.6.0 until `ragrig-gpui 0.6.1` is on crates.io (`candidate versions
found which didn't match: 0.5.1, 0.5.0, 0.2.0`).  Publish `ragrig 1.3.0`,
then `ragrig-gpui 0.6.1`, then remove scico's development
`[patch.crates-io]` and re-run the dry run.

## v0.5.1 — 2026-10-27

### Changed

- **Import PDFs.** The Library's **Add PDFs** action is now **Import PDFs**.
  Copies always land in the canonical `<workspace>/corpus/`; importing now
  requires a workspace and reports a clear error when none is configured. That
  folder becomes the active corpus for extraction and indexing even when
  Settings names another corpus.
- **Workspace entry opens only the paper.** Creating or entering a workspace
  no longer opens its `.bib`. A new **BibTeX** action in the Library header
  opens the raw bibliography on demand, and the workspace instruction now says
  “Import PDFs into the library and chat about them.”
- **Managed GROBID auto start.** Settings → External services → managed
  container now has an **Auto start** switch. When it is on, Scico starts the
  managed container (pulling the image when necessary) as the app opens. The
  preference is persisted in `settings.json`.

### Fixed

- **GROBID waits for the bibliography tab.** GROBID no longer updates the
  bibliography while the `.bib` is open; the Library explains why and disables
  the GROBID button. The new BibTeX action is disabled while a GROBID run is
  active, and an import that lands while the tab is open tells the user to
  close it before parsing.
- **BibTeX jumps place the key two lines below the viewport top.** Opening an
  entry from a reference row's pencil now corrects the editor's minimal
  bottom-edge auto-scroll after the first paint, instead of leaving the entry
  at the bottom of the viewport.

### Quality

Release validation on 2026-10-27 (Cargo 1.99.0, rustc 1.99.0).

**Test coverage.** `cargo llvm-cov --all-features --summary-only` selects the
zorite backend because `build.rs` prefers it when both editor features are
enabled (doctests are not measured on stable Rust):

| Metric | Covered | Total | Cover |
| --- | ---: | ---: | ---: |
| Regions | 9,668 | 14,590 | 66.26% |
| Functions | 626 | 1,087 | 57.59% |
| Lines | 5,675 | 8,356 | 67.92% |

The default gpuikit build, which is what ships, measures higher:
`cargo llvm-cov --summary-only` reports 70.42% regions, 63.21% functions and
71.59% lines (78 unit tests + 1 integration test). The least covered areas
are the new Library UI (`src/library.rs` 45.88%; its pure helpers are unit
tested) and the agent (`src/agent.rs` 38.80%).

**Tests.** `cargo test --all-features`: 65 unit tests + 1 integration test —
**66 passed, 0 failed**. The default feature set (`cargo test --all-targets`,
gpuikit): 78 unit tests + 1 integration test — **79 passed, 0 failed**. No
doctests. One earlier run, executed concurrently with the ragrig-gpui suite,
had a single transient failure; it did not reproduce in three consecutive
isolated reruns or a repeated parallel run.

**Checks.** `cargo fmt --check`, `cargo check --all-targets --all-features`,
`cargo clippy --all-targets --all-features -- -D warnings`,
`cargo doc --no-deps --all-features`, and `cargo package --list` are green.

**Release blocker.** `cargo publish --dry-run` cannot verify scico 0.5.1
until `ragrig-gpui 0.5.1` is on crates.io (`candidate versions found which
didn't match: 0.5.0, 0.2.0`). `grobid-bibtex 0.3.1` is also still unreleased;
scico 0.5.1 must be packaged after both, or it will keep resolving
`grobid-bibtex 0.3.0`, which lacks the 255-byte PDF filename cap. Publish
grobid-bibtex 0.3.1, then ragrig-gpui 0.5.1, then remove scico's
`[patch.crates-io]`, update the lock, and re-run the dry run.

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
