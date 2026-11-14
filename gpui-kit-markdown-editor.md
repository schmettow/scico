# Evaluation: `gpui-kit-markdown-editor` as an editor backend

Evaluated 2026-11-13, integrated and re-evaluated 2026-11-14 against
`wabzqem/gpui-kit-markdown-editor` 0.2.0 (lib name `markdown_editor`,
Apache-2.0, ~11.4k lines, published 2026-11-07).

**Verdict: a viable experimental third backend, not a replacement.** The
`EditorSurface` seam absorbed the integration, and the WYSIWYG surface removes
the preview machinery. The block model still cannot address canonical-source
bytes, which keeps the RAG selection roundtrip off. Scico ships it behind
`gpuikit-markdown-editor`; `UPSTREAM.md` (UM001–UM004) records what would
close the gaps.

## What the 2026-11-14 integration settled

- **It did not build at all.** 0.2.0 initializes `BlockNode::ListItem` without
  `task_marker`, a field `gpui-base 0.9.1` added; `cargo check` fails before
  Scico compiles. `vendor/gpui-kit-markdown-editor` is 0.2.0 plus
  `task_marker: None`, selected by `[patch.crates-io]`. Upstream has no newer
  release (its `main` only changed the README after 0.2.0).
- **Integrated behind its own feature.** `build.rs` selects `editor_block`
  before `zorite` and `gpuikit`, so `--all-features` builds this backend. The
  adapter is `src/editor_block.rs`; the cfg seams are the same ones the other
  backends already use.
- **What works:** editing and saving, undo/redo, IME, tables, links (opened by
  the host), font family/size, scroll retention, native caret insertion, and a
  guarded replacement that rebuilds the canonical source under the digest
  guard.
- **What cannot work yet:** a selection as canonical bytes (so Focus/Edit/
  Enrich are off), host range decorations for an armed edit, a separate
  preview/raw mode, and byte-exact saves. Reveal and the caret line are
  top-level-block granular. The canonical source normalizes soft line breaks,
  so opening and saving a hand-wrapped `.bib` or prompt rewrites it.
- **The zorite lessons do not transfer.** `DisplayMap`/`reveal_range` and
  `offset_screen_top` solve pixel-to-row and deep-reveal problems for a source
  editor. This model has no source offset to feed them; the adapter scans the
  serialized source to find top-level blocks instead.
- **Performance is unmeasured in Scico.** The upstream headless figures are
  plausible, but no typing/reveal budget test exists for this backend yet. A
  regression guard like `tests/zorite_performance.rs` is the follow-up.

## What it is

A block-based WYSIWYG Markdown editor built on `gpui-kit` 0.9's public
`base::text::editor_host` seam:

- `BlockEditorModel`: headless document, stable `BlockId`s, normalised inline
  marks, invertible transactions, intent-coalescing undo.
- `BlockEditorState`: focus, IME via `EntityInputHandler`, keybindings under
  the `BlockEditor` context, `Changed { revision, origin }` events.
- `BlockEditor`: renders model → `BlockNode` with per-node revision caching,
  top-level culling, and a Fenwick prefix-sum height index.
- Host API that closely mirrors scico's roundtrip shape: `source_snapshot`,
  `capture_leaf_range` → opaque `CapturedRange`, `is_capture_current`,
  `replace_captured`, `invalidate_captures`, `set_readonly`, `set_source`.

## Fit against the roundtrip contract

| Requirement | Result | Detail |
|---|---|---|
| Address text by source character range | **No** | Block/cell positions only. `source_offset` / `position_at_source_offset` are explicitly deferred (their D1). |
| Guarded replacement | Yes | `CapturedRange` is owner + revision + selection epoch pinned, validated by `replace_captured`, one undoable transaction, advances over inserted text. Scico's byte-addressed replacement reloads the source and therefore clears undo. |
| Capture a selection as bytes | **No** | `Selection` is block/cell based. The session journal and `ReplacementTarget` speak byte offsets. |
| Reveal an offset from the Library | **Partial** | `reveal_caret` only. The Scico adapter maps a byte to its top-level block and reveals that block's first leaf. |
| Host range decorations (`mark_editor_range`) | **No** | No `RangeDecorationCollection`, gutter, or overview-ruler equivalents. |
| Byte-exact save | **No** | Canonicalising serializer: normalises emphasis markers, list tightness (once), drops empty paragraphs, resolves reference links, rewrites highlight as `<mark>`, and collapses soft line breaks. Their docs: "lossless in meaning, not byte-exact". |
| Undo | Yes, better | Intent coalescing (typing, word delete, IME brackets, break on caret move). Worth borrowing conceptually. |
| IME | Yes | `EntityInputHandler` over the caret leaf. |
| Accessibility | Partial | One multiline text node plus top-level blocks; nested lists, table cells, inline roles not exposed; screen-reader behaviour self-declared unverified. |
| Performance | Plausible, unproven | 1–1.8 ms/frame at 1200 blocks headless; nested long lists and tables laid out in full (their Phase 5 partly stopped). |
| Licence / maturity | Apache-2.0 / early | 0.2.0, three weeks old, one author, macOS bindings untested in CI. |
| Builds out of the box | **No** | `BlockNode::ListItem` lacks the `task_marker` field added in `gpui-base 0.9.1`. The feature's `syntect`/`tree-sitter` forwarding does work; the field is patched in `vendor/`. |

## The trade

Gain: the editor and preview collapse into one surface; the preview block map,
the scroll-sync state machine, and the preview snapshot are unnecessary. That
is the largest single simplification available to scico, and the roundtrip
traits kept the integration to one adapter module plus cfg branches.

Lose: "the passage that was at bytes 120..480 three edits ago". The protocol,
digest guard, journal, and selection capture are offset-addressed.
`CapturedRange` answers "is what I captured still there and may I replace it",
not "where is region X now". A full switch would still mean rewriting
`EditorSurface`, `Roundtrip`, and the journal schema — and giving up a
capability we have. The experiment therefore stays opt-in.

## Triggers to revisit

All four must hold:

1. `source_offset` / `position_at_source_offset` (their D1) shipped.
2. A host range-decoration API shipped (or gutter/overview decorations are
   enough for the edit-target markers).
3. A byte-exact or stable-roundtrip save mode, or explicit acceptance of
   canonicalising saves.
4. A second maintainer and CI covering Linux and macOS bindings.

## When revisiting

- The prototype now exists behind `editor_block` and its own feature; keep
  `gpuikit` and `zorite` untouched when extending it.
- Once source offsets ship, replace the block-granular `SourceMap` with the
  upstream mapping and enable selection capture; keep the `EditorSurface`
  adapter and the journal schema.
- Add a performance guard like `tests/zorite_performance.rs` before considering
  this backend for daily use.
- Re-run the prompt and bibliography tests: they currently normalize text and
  line numbers because of the canonicalising serializer.

## Ideas worth adopting regardless

- Opaque captures with owner + revision + selection epoch.
- Intent-coalesced undo instead of character-level grouping.
- Stale-request fencing via captures rather than generation counters.
- Clipboard policy split (editor owns vs host owns) for literal Markdown tags.
