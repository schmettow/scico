# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- **Settings → Overview**: profile picker and name, per-section pending-change
  counts, and a single action row with **Apply**, **Profile Defaults**,
  **Update Profile**, and **Load profile**. Apply is disabled while nothing is
  pending and violet when a rebuild is due.
- Field-level unsaved marks: RAG settings that differ from the running agent are
  bold violet, their sections and pages carry `●` in the sidebar, and changed
  sections show an “Unsaved changes” line.
- Adjustable settings column: drag the divider on the right; the width is
  persisted with the other settings.
- `RagSettings::reset_to_profile_defaults()` backs **Profile Defaults** (reload
  the selected profile, or the built-in defaults while keeping the workspace
  and bibliography).

### Changed

- The RAG page now contains only Chat, Retrieval, Parsing and chunking, and
  Memory. Profiles and Apply moved to Overview and are no longer a section in
  the page hierarchy.
- External viewer and path fields fit the resizable controls column; **Browse…**
  no longer clips at narrow widths.

### Fixed

- The `Copy @key` action, the vector-store component, and the chunker component
  rendered blank icons. Copy now uses the `Copy` glyph; the store and chunker
  use `Layers` and `Columns3`, whose SVGs the rasteriser supports.
