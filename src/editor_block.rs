//! Adapter for `gpui-kit-markdown-editor`.
//!
//! The block editor addresses text by stable block and cell positions, not by
//! canonical-Markdown byte offsets. This module maps the two at top-level block
//! granularity so the Library can reveal an entry and the status bar can show a
//! caret line. Operations that need an exact source selection (the roundtrip
//! protocol, range decorations, the preview) stay unavailable; see
//! `UPSTREAM.md` and `gpui-kit-markdown-editor.md`.

use std::ops::Range;

use gpui_kit::{App, AppContext as _, Context, Entity, Window};
use markdown_editor::{BlockEditorState, BlockId, BlockPosition, BlockSelection};

pub type EditorState = BlockEditorState;

/// The editor entity used by [`Document`](crate::document::Document).
pub fn new_editor(window: &mut Window, cx: &mut App, source: &str) -> Entity<EditorState> {
    cx.new(|cx| EditorState::new(window, cx).with_source(source))
}

/// Source spans of the editor's top-level blocks, in document order.
///
/// The editor's serializer is the source of truth: each top-level block is
/// located by scanning the canonical source for its first non-empty leaf text.
/// This is block-granular, which is enough for revealing a Library entry and
/// for a caret line number, but it cannot address a character inside a block.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    spans: Vec<Range<usize>>,
}

impl SourceMap {
    pub fn new(editor: &EditorState, source: &str) -> Self {
        let model = editor.model();
        let doc = model.document();
        let tops: Vec<BlockId> = doc.root().children().map(|block| block.id()).collect();
        let mut spans: Vec<Range<usize>> = Vec::with_capacity(tops.len());
        let mut cursor = 0usize;
        for top in tops {
            let anchor = first_leaf_text(&doc, top);
            let found = anchor.and_then(|text| locate(source, cursor, text));
            let start = found.map_or(cursor, |(at, _)| line_start(source, at));
            let end = found.map_or(start, |(at, len)| at + len);
            spans.push(start..end.max(start));
            cursor = end.max(start);
        }
        if let Some(last) = spans.last_mut() {
            last.end = source.len().max(last.end);
        }
        Self { spans }
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub fn span(&self, index: usize) -> Option<Range<usize>> {
        self.spans.get(index).cloned()
    }

    /// The top-level block that contains `byte`.
    pub fn index_for(&self, byte: usize) -> Option<usize> {
        if self.spans.is_empty() {
            return None;
        }
        let index = self
            .spans
            .partition_point(|span| span.start <= byte)
            .saturating_sub(1);
        Some(index.min(self.spans.len() - 1))
    }
}

/// The caret as a byte offset in the canonical source.
///
/// Inline markers are not represented in `BlockPosition`, so the result is
/// block-relative and can drift from the exact source offset inside a formatted
/// run. It is stable enough for a line/column display and a journal restore.
pub fn cursor_byte(editor: &EditorState, source: &str) -> usize {
    let model = editor.model();
    let Some(caret) = model.caret() else {
        return 0;
    };
    let doc = model.document();
    let Some(top) = top_level_index(&doc, caret.block) else {
        return 0;
    };
    let map = SourceMap::new(editor, source);
    let Some(span) = map.span(top) else {
        return 0;
    };
    let Some(leaves) = top_leaves(&doc, top) else {
        return span.start;
    };
    let preceding = leaves
        .iter()
        .take_while(|leaf| **leaf != caret.block)
        .filter(|leaf| {
            doc.block(**leaf)
                .and_then(|block| block.text())
                .is_some_and(|text| !text.is_empty())
        })
        .count();
    let text = doc
        .block(caret.block)
        .and_then(|block| block.text())
        .unwrap_or_default();
    let start = locate_nth(&source[span.start..span.end], text, preceding)
        .map_or(span.start, |at| span.start + at);
    (start + caret.offset).min(source.len())
}

/// Move the caret to the top-level block containing `byte` and reveal it.
pub fn reveal(editor: &mut EditorState, source: &str, byte: usize, cx: &mut Context<EditorState>) {
    let map = SourceMap::new(editor, source);
    let Some(top) = map.index_for(byte) else {
        return;
    };
    let Some(span) = map.span(top) else {
        return;
    };
    let doc = editor.model().document();
    let Some(leaves) = top_leaves(&doc, top) else {
        return;
    };
    let Some(first) = leaves.first() else {
        return;
    };
    let text = doc
        .block(*first)
        .and_then(|block| block.text())
        .unwrap_or_default();
    let offset = if leaves.len() == 1 {
        floor_char_boundary(text, byte.saturating_sub(span.start))
    } else {
        0
    };
    if editor
        .set_selection(
            BlockSelection::caret(BlockPosition::new(*first, offset)),
            cx,
        )
        .is_ok()
    {
        editor.reveal_caret(cx);
    }
}

/// Insert at the native caret as one undoable transaction.
///
/// This is the block editor's exact operation; it does not go through a source
/// byte range.
pub fn insert_at_caret(editor: &mut EditorState, text: &str, cx: &mut Context<EditorState>) {
    match editor.capture_caret() {
        Ok(mut capture) => {
            if let Err(error) = editor.replace_captured(&mut capture, text, cx) {
                eprintln!("markdown editor rejected an insertion at the caret: {error:?}");
            }
        }
        Err(error) => eprintln!("markdown editor has no caret to insert at: {error:?}"),
    }
}

/// Replace a byte range by loading the rebuilt canonical source.
///
/// The block editor cannot address arbitrary source bytes, so this is the one
/// operation that is exact in text but coarse in history: it reparses and
/// clears undo. The caller keeps the digest guard, so a stale replacement is
/// still rejected before it reaches the editor.
pub fn replace_byte_range(
    editor: &mut EditorState,
    source: &str,
    range: Range<usize>,
    text: &str,
    cx: &mut Context<EditorState>,
) {
    let start = floor_char_boundary(source, range.start.min(source.len()));
    let end = ceil_char_boundary(source, range.end.min(source.len()).max(start));
    let mut updated = String::with_capacity(source.len() - (end - start) + text.len());
    updated.push_str(&source[..start]);
    updated.push_str(text);
    updated.push_str(&source[end..]);
    if let Err(error) = editor.set_source(&updated, cx) {
        eprintln!("markdown editor rejected the rebuilt source: {error}");
    }
}

fn top_level_index(doc: &markdown_editor::DocumentRef<'_>, id: BlockId) -> Option<usize> {
    let root = doc.root().id();
    let tops: Vec<BlockId> = doc.root().children().map(|block| block.id()).collect();
    let mut current = id;
    loop {
        if let Some(index) = tops.iter().position(|top| *top == current) {
            return Some(index);
        }
        let parent = doc.block(current)?.parent()?.id();
        if parent == root {
            return None;
        }
        current = parent;
    }
}

fn top_leaves(doc: &markdown_editor::DocumentRef<'_>, top: usize) -> Option<Vec<BlockId>> {
    let id = doc.root().children().nth(top)?.id();
    Some(
        doc.leaves()
            .filter(|leaf| is_within(doc, leaf.id(), id))
            .map(|leaf| leaf.id())
            .collect(),
    )
}

fn is_within(doc: &markdown_editor::DocumentRef<'_>, id: BlockId, ancestor: BlockId) -> bool {
    let mut current = Some(id);
    while let Some(block) = current {
        if block == ancestor {
            return true;
        }
        current = doc
            .block(block)
            .and_then(|block| block.parent())
            .map(|parent| parent.id());
    }
    false
}

fn first_leaf_text<'a>(doc: &'a markdown_editor::DocumentRef<'a>, top: BlockId) -> Option<&'a str> {
    doc.leaves()
        .filter(|leaf| is_within(doc, leaf.id(), top))
        .filter_map(|leaf| leaf.text())
        .find(|text| !text.is_empty())
}

fn line_start(source: &str, at: usize) -> usize {
    source[..at].rfind('\n').map_or(0, |newline| newline + 1)
}

/// Locate a leaf's text in the source, falling back to its first token.
///
/// The editor removes soft line breaks from native text while the canonical
/// source keeps them, so a whole wrapped paragraph rarely matches verbatim.
/// Its first token still does, which is enough to find the block start.
fn locate(source: &str, from: usize, text: &str) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }
    if let Some(at) = source.get(from..)?.find(text) {
        return Some((from + at, text.len()));
    }
    let token = text.split_whitespace().next()?;
    source
        .get(from..)?
        .find(token)
        .map(|at| (from + at, token.len()))
}

fn locate_nth(haystack: &str, needle: &str, n: usize) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    if let Some(at) = haystack.match_indices(needle).nth(n).map(|(at, _)| at) {
        return Some(at);
    }
    let token = needle.split_whitespace().next()?;
    haystack.match_indices(token).nth(n).map(|(at, _)| at)
}

fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn ceil_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{IntoElement, Render, TestAppContext, VisualTestContext, div};

    struct Harness(Entity<EditorState>);

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn mount<'a>(
        cx: &'a mut TestAppContext,
        source: &str,
    ) -> (
        Entity<Harness>,
        Entity<EditorState>,
        &'a mut VisualTestContext,
    ) {
        cx.update(gpui_kit::init);
        let (harness, cx) = cx.add_window_view(|window, cx| {
            Harness(cx.new(|cx| EditorState::new(window, cx).with_source(source)))
        });
        let editor = harness.read_with(cx, |harness, _| harness.0.clone());
        (harness, editor, cx)
    }

    #[gpui_kit::test]
    fn the_source_map_finds_a_wrapped_bibliography_entry(cx: &mut TestAppContext) {
        let source = "@article{First2020,\n  title = {First},\n}\n\n@book{Kano2025,\n  title = {Second},\n}\n";
        let (_harness, editor, cx) = mount(cx, source);
        let canonical = cx.update(|_, cx| editor.read(cx).source());
        let byte = canonical.find("@book").expect("@book");
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| reveal(editor, &canonical, byte, cx));
        });
        let (spans, cursor) = cx.update(|_, cx| {
            let editor = editor.read(cx);
            let map = SourceMap::new(editor, &canonical);
            let spans: Vec<_> = (0..map.len()).filter_map(|index| map.span(index)).collect();
            (spans, cursor_byte(editor, &canonical))
        });
        eprintln!("canonical: {canonical:?}");
        eprintln!("spans: {spans:?}; @book at {byte}; cursor {cursor}");
        assert_eq!(cursor, byte, "reveal must land on the @book line");
    }
}
