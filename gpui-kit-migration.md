# gpui-kit migration log and plan

`gpui-kit` moves quickly and every minor release has carried breaking changes.
This file is the standing plan: one entry per upgrade, current entry first,
with the decisions we made and the verification we ran.

## Rules

- One task per commit; each commit must build and pass its suite.
- Tests before implementation where behaviour changes.
- Regenerate `Cargo.lock` deliberately. The checked-in lockfile is intentional
  (see `DEVELOPER_NOTES.md`); never blanket-update it.
- Verify three configurations: `--no-default-features --features gpuikit`,
  `--no-default-features --features zorite`, and `--all-features` (build.rs
  selects zorite when both are on).
- `ragrig-gpui` is path-patched into scico and pins the same `gpui-kit`.
  Upgrade both in the same working session.

## Entry 1: 0.7.3 → 0.9.1 (in progress, 2026-11-13)

Status: **0.9.1 adopted.** Adoption backlog tracked below.

### Breaking changes and our exposure

| Change | Exposure | Action |
|---|---|---|
| `TextViewStyle` builder-only | none: the base type is builder-only, but `ragrig-gpui` imports the **component** type, which keeps its public `is_dark` field | none (initial assumption was wrong; verified in 0.9.1 source) |
| Theme structs `#[non_exhaustive]` | none: scico only reads/clones the theme | none |
| `ScrollWheelEvent` → `ScrollEvent` / `on_scroll` | two production wheel handlers, six test dispatches | Migrate in task 2 |
| `Window::text_system()` returns `&Rc<WindowTextSystem>` | scico uses `App::text_system()` | none |
| `InputBaseState::text()` returns `&Rope` | scico uses `value()` on the gpuikit path | none; do not assume `String` |
| `DragHandle`, `TabVariant`, `Select`, `SelectionFormat`, `InvocationKind`, `set_line_number`, `set_searchable` | no matches in either crate | none |
| gpui-pre 0.3.12, gpui-fast 0.1.14 (`resvg ^0.46`) | lockfile baseline changes | deliberate lockfile regeneration |
| `visible_row_range()` is in **buffer lines**, not display rows | the old `source_top_line` walked `range_to_bounds` from a pixel estimate; under 0.9's layout it reported line 0 after a wheel scroll and the sync snapped the preview back | read `start` directly; the display map is used only for the preview-to-editor mapping |
| ScrollBounce scrolls via `ScrollbarHandle::set_offset`, bypassing `ListState`'s scroll handler | `set_scroll_handler` no longer fires on wheel input, so the preview-to-editor direction went blind | sample the preview top once per frame in `editor_gpuikit`; flagged upstream |

### Adoption backlog

- [x] Task 1 — plan, version bump, `TextViewStyle` check (no change needed).
- [ ] Task 2 — wheel handler on `on_scroll` / `ScrollEvent`; suites green.
- [x] Task 3 — `DisplayMap` + `reveal_range`; delete `source_top_line`,
      `PreviewScrollTarget`, and the probing loop. One `list_state()` read
      remains, sampling the preview top once per frame (see below).
- [ ] Task 4 — `InputEditsEvent` in `subscribe_editor`; drop the change-event
      revision counter.
- [ ] Later — gutter/overview decorations for pending edits and enrichment
      targets.
- [ ] Later — `TextView::on_task_toggle` for clickable task lists.
- [ ] Later — preview theming via `TextViewStyle::from_theme`,
      `with_code_block`, `with_table`; retest wheel-over-code behaviour.
- [ ] Later — `Editor::set_auto_close(false)` / `set_smart_indent(false)`.
- [ ] Later — `Theme::cursor_blink`, `set_accessibility`, `window_radius`.

### Verification matrix

| Configuration | Suite | Result |
|---|---|---|
| gpuikit | `cargo test --no-default-features --features gpuikit` | see commit |
| zorite | `cargo test --no-default-features --features zorite` | see commit |
| all features | `cargo test --all-features` | see commit |

### Rollback

Revert the dependency bump and the per-task commits in reverse order. The
scroll-sync removal is the only change that also deletes state fields; the
`DisplayMap` commit is self-contained and can be reverted alone.

### Open upstream asks

- `ListView`/`TextView` wheel scrolling should invoke `ListState`'s scroll
  handler, or the view should expose a scroll event, so hosts do not have to
  sample `logical_scroll_top()` during render.
- A documented scroll-position accessor on `TextViewState` would remove the
  last `list_state()` read from scico's render path.

## Entry 0: 0.6.x → 0.7.3 (historical)

Adopted 0.7.3 with `gpui-pre` 0.3.9 and `gpui-fast` 0.1.11. The checked-in
lockfile exists because the optional gpui-fast branch pins
`cocoa =0.26.0` and `unicode-properties =0.1.3`; a stale lockfile fails with a
cocoa/unicode conflict.
