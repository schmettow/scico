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
