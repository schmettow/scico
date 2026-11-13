# Evaluation: `gpui-kit-markdown-editor` as an editor backend

Evaluated 2026-11-13 against `wabzqem/gpui-kit-markdown-editor` 0.2.0
(lib name `markdown_editor`, Apache-2.0, ~11.4k lines, published 2026-11-07).

**Verdict: right architecture, wrong moment.** It is not a drop-in replacement
for scico's source editor, and two gaps are structural rather than cosmetic.
Keep it under observation; the roundtrip traits in `ragrig-gpui` are the seam
that will make an eventual third backend cheap.

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
| Guarded replacement | Yes | `CapturedRange` is owner + revision + selection epoch pinned, validated by `replace_captured`, one undoable transaction, advances over inserted text. |
| Capture a selection as bytes | **No** | `Selection` is block/cell based. The session journal and `ReplacementTarget` speak byte offsets. |
| Reveal an offset from the Library | **No** | `reveal_caret` only. |
| Host range decorations (`mark_editor_range`) | **No** | No `RangeDecorationCollection`, gutter, or overview-ruler equivalents. |
| Byte-exact save | **No** | Canonicalising serializer: normalises emphasis markers, list tightness (once), drops empty paragraphs, resolves reference links, rewrites highlight as `<mark>`. Their docs: "lossless in meaning, not byte-exact". |
| Undo | Yes, better | Intent coalescing (typing, word delete, IME brackets, break on caret move). Worth borrowing conceptually. |
| IME | Yes | `EntityInputHandler` over the caret leaf. |
| Accessibility | Partial | One multiline text node plus top-level blocks; nested lists, table cells, inline roles not exposed; screen-reader behaviour self-declared unverified. |
| Performance | Plausible, unproven | 1–1.8 ms/frame at 1200 blocks headless; nested long lists and tables laid out in full (their Phase 5 partly stopped). |
| Licence / maturity | Apache-2.0 / early | 0.2.0, three weeks old, one author, macOS bindings untested in CI. |
| Builds out of the box | Unverified | Features forward to `gpui-kit/syntect` and `gpui-kit/tree-sitter`, which gpui-kit 0.9.1 does not expose as its own features. Run `cargo check` before betting on it. |

## The trade

Gain: the editor and preview collapse into one surface; `document.rs`'s block
list, the scroll-sync state machine, and the preview snapshot all disappear.
That is the largest single simplification available to scico.

Lose: "the passage that was at bytes 120..480 three edits ago". The protocol,
digest guard, journal, and Library reveal are offset-addressed. `CapturedRange`
answers "is what I captured still there and may I replace it", not "where is
region X now". Migration means rewriting `EditorSurface`, `Roundtrip`, and the
journal schema — and giving up a capability we have.

## Triggers to revisit

All four must hold:

1. `source_offset` / `position_at_source_offset` (their D1) shipped.
2. A host range-decoration API shipped (or gutter/overview decorations are
   enough for the edit-target markers).
3. A byte-exact or stable-roundtrip save mode, or explicit acceptance of
   canonicalising saves.
4. A second maintainer and CI covering Linux and macOS bindings.

## When revisiting

- Prototype behind a third `build.rs` cfg (`editor_block`) and its own feature.
  Do not touch `gpuikit` or `zorite`.
- Reuse `EditorSurface`; add an adapter that translates the journal's byte
  offsets through `CapturedRange` rather than redefining the journal.
- Measure typing latency on a real 10k-word manuscript, not the headless bench.

## Ideas worth adopting regardless

- Opaque captures with owner + revision + selection epoch.
- Intent-coalesced undo instead of character-level grouping.
- Stale-request fencing via captures rather than generation counters.
- Clipboard policy split (editor owns vs host owns) for literal Markdown tags.
