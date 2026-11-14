# Upstream notes

Requests and gaps in the crates Scico builds on. Each entry has an ID, the
upstream crate, the request, why it matters, and the workaround used here until
the upstream change lands.

## Ragrig

### US001 — Expose bounded parameters as ranges

**Upstream:** `ragrig` configuration types (and the settings UI in
`ragrig-gpui`).

**Request.** When a parameter has a natural lower and upper bound, publish the
bound together with the value instead of leaving it to every front end. For
example, replace a bare `similarity_threshold: f64` with a descriptor such as

```rust
Parameter {
    value: 0.5,
    range: 0.0..=1.0,
    step: 0.01,
    default: 0.5,
}
```

or, minimally, expose `MIN`/`MAX`/`STEP` constants next to the configuration
types. The parameters that need it include:

| Parameter | Natural range |
|---|---|
| `similarity_threshold` | `0.0 ..= 1.0` |
| `top_k` | `1 ..=` implementation limit |
| `temperature` | `0.0 ..= 2.0` |
| `chunk_size` | `1 ..=` model/parser limit |
| `chunk_overlap` | `0 ..= chunk_size` |
| `max_tokens`, `context_tokens` | `1 ..=` model window |

**Why.** The CLI, the settings dialog, and hand-written config files currently
repeat the same bounds and step sizes, and they drift. Validation happens late
(`RagSettings::validate()` after the user has typed a value), so a host can
accept a number in the widget that the engine will reject on Apply. A range
descriptor lets a front end validate at the edge, render the right stepper, and
generate documentation from one source of truth.

**Current workaround.** `ragrig-gpui` carries `NumberFieldOptions { min, max,
step }` beside every numeric setting and validates again on commit. This is the
duplication the upstream range would remove.

**Status.** Open.

## gpui-kit-markdown-editor

Scico's experimental `gpuikit-markdown-editor` backend is the first host of
this crate. The items below are what keeps it from reaching parity with the
source editors.

### UM001 — Compile against gpui-base 0.9.1

**Upstream:** `gpui-kit-markdown-editor` 0.2.0.

**Request.** Release a version that compiles against `gpui-base 0.9.1`.
0.2.0 initializes `BlockNode::ListItem` without the `task_marker` field that
0.9.1 added, so `cargo check` fails before any Scico code is built. The
feature-gated `syntect`/`tree-sitter` features also assume a gpui-kit release
whose features now exist, so a fresh compatibility release is enough.

**Why.** Scico links one `gpui-kit`/`gpui-base` generation at a time. Pinning
an older base for this dependency would split the UI's element types.

**Current workaround.** `vendor/gpui-kit-markdown-editor` is 0.2.0 plus
`task_marker: None`, selected by `[patch.crates-io]`. The patch is
development-only; the packaged manifest keeps the registry dependency. Remove
the vendor copy when a compatible release is published.

**Status.** Open. Upstream's last release is 0.2.0 (2026-11-07); `main` has
only README changes after it.

### UM002 — Address positions and selections in canonical-source bytes

**Request.** Public `source_offset(BlockPosition) -> usize` and
`position_at_source_offset(usize) -> Option<BlockPosition>` (upstream D1), plus
a way to read a selection as a canonical-source char range.

**Why.** Scico's whole roundtrip is byte/char addressed: the session journal,
`ReplacementTarget`, the digest guard, the Library reveal, and range
highlighting all speak canonical Markdown offsets. `BlockPosition` is relative
to a block's flat text, which removes inline markers, so a caret or selection
cannot be converted exactly. Without this, Scico cannot offer Focus/Edit/Enrich
on this backend and replaces a marked passage by reloading the whole source
(which clears undo).

**Current workaround.** `src/editor_block.rs` maps at top-level block
granularity for reveal and the caret line, inserts at the caret through
`capture_caret`, and rebuilds the source for a guarded replacement.

**Status.** Open. Their design doc lists source offsets as deferred D1.

### UM003 — Host range decorations

**Request.** A `RangeDecorationCollection` equivalent, or gutter/overview
markers, addressed by block range or source bytes.

**Why.** Scico marks the passage an edit will replace, and the marker must
survive focus moving to the chat composer. `BlockEditorStyle` only exposes
caret and selection colours.

**Current workaround.** `mark_editor_range` returns an inert handle on this
backend; the **Direct edit** chip still names the target. The editor's own
native selection is used while it has focus, but it collapses when focus
moves away.

**Status.** Open.

### UM004 — A stable or byte-exact save mode

**Request.** A serialization mode that preserves soft line breaks and is stable
across re-import, or an explicit API to read the original source when the
document was not edited.

**Why.** The block model removes soft wraps and normalizes inline markers on
import, and `source()` serializes the normalized model. Opening a hand-wrapped
`.bib` or prompt rewrites it as one line per paragraph on save; line numbers
and diffs change even though the meaning is preserved. Their documentation
calls the result "lossless in meaning, not byte-exact".

**Current workaround.** Scico accepts the canonical form for this backend and
compares prompt text token by token in tests. The digest guard still protects
agent edits, but a plain Save can reflow a file.

**Status.** Open, by design.
