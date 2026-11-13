# Roadmap

Scico is a scientific Markdown editor with an integrated RAG agent and
reference management. The current prototype is functional; the API is not
stabilised.

## Now

- Scico branding is in place: banner, desktop mark, welcome state, and the
  assistant/chat/send marks. Linux now installs a `.desktop` entry with the
  hicolor icons on first launch. Next: ship the macOS `.icns` and Windows
  `.ico` packaging assets.
- Documentation is split: `README.md` speaks to writers and students,
  `DEVELOPER_NOTES.md` keeps the architecture and protocol notes, and
  `UPSTREAM.md` tracks requests to Ragrig.
- Keep the roundtrip experience solid across both editor backends: selection
  capture, guarded write-back, Chat edit mode, and complete crash-safe session
  restore.
- Polish the `gpuikit` writing surface: collapsible preview, two-way
  block-level synchronized scrolling, theme-following syntax highlighting, and
  persisted font family/size with Ctrl+wheel zoom are in place. Refine parser
  parity for exotic Markdown next.
- Keep the editor usable without the mouse: smooth scrolling, word-wise
  movement, and reliable pane toggles.
- The left panel is now the **Workspace** super container, with **New
  workspace** and **Open workspace** at the top and the **Library** as one
  pipeline. Next: add further pipelines (web corpora, notes, or a second index)
  under the same workspace shell.

- Edit and Enrich targets now keep a persistent range decoration, and the
  target document is read-only while the agent writes (`gpuikit`). Next: add a
  cancel path for an armed edit and use a per-range guard if gpuikit offers one.
- The Chat prompt is file-based: the full `Chat.md` is compiled in, can be
  exported into the workspace, edited in the editor, and overridden per
  profile. Refine the prompt alongside protocol changes.
- Enrich selection now runs retrieval on the marked passage itself. Next:
  compare enriched retrieval quality against ordinary selection context and
  expose the retrieved sources beside the replacement.
- **First-run setup** now walks through the embedding model, chat provider and
  model, token limits, retrieval, and optional GROBID, with Docker/existing
  container choices and model-aware token suggestions. The embedding step still
  assumes a local Ollama model, while the standard settings are OpenRouter;
  next, add an embedding-provider choice there so the guide and the defaults
  agree. Then exercise it with a genuinely empty settings directory on Windows
  and macOS and add a short "what next" tour after finishing.
- **External services** now manages three local services: GROBID, Ollama, and
  llama.cpp. Each can use an existing server, start and stop a hand-configured
  container by name, or run a managed Docker container; Ollama models are pulled
  into a volume from the page, and llama.cpp accepts a Hugging Face model or a
  local `.gguf`. Verified end to end on Linux for GROBID and for an existing
  Intel SYCL Ollama container. Next: exercise the managed containers on Windows
  and macOS; the GROBID full image runs emulated on Apple silicon.
- The Chat status bar switches Chat memory and hybrid retrieval from the
  component menu. Next: surface the running memory strategy and retrieval
  mode in the transcript meta so saved sessions explain how an answer was
  produced.
- Paths are project-relative: the process working directory follows the open
  workspace (home without one), and the bibliography records PDF links
  relative to the workspace. Next: offer a one-time migration that rewrites
  absolute links in existing bibliographies.

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

- Publish `scico` and `ragrig-gpui` from the `gpui_kit` and `dev` branches,
  keeping the crates.io checklist in `CHANGELOG.md` green.
- Reach functional parity with the earlier egui prototype and retire it.
