//! Documents, tabs, and file operations.

use std::cell::RefCell;
#[cfg(editor_zorite)]
use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
#[cfg(editor_zorite)]
use std::sync::Arc;

#[cfg(editor_zorite)]
use gpui_kit::EntityInputHandler as _;
#[cfg(any(editor_zorite, editor_gpuikit))]
use gpui_kit::RenderImage;
use gpui_kit::{
    App, AppContext as _, ElementId, Entity, FocusHandle, Focusable as _, Hsla, ScrollHandle,
    SharedString, Window,
};
#[cfg(any(editor_zorite, editor_gpuikit))]
use image::Frame;
use ragrig_gpui::editor::{EditorSurface, byte_to_char_index, char_range_to_bytes};

#[cfg(editor_gpuikit)]
use gpui_kit::base::text::TextViewState;
#[cfg(editor_gpuikit)]
pub use gpui_kit::component::input::EditorState;
#[cfg(editor_zorite)]
pub use zorite_editor::{EditorState, SyntaxStyle};
#[cfg(editor_gpuikit)]
/// Placeholder for zorite's theme type; the gpuikit editor follows the app theme.
#[derive(Clone, Debug, Default)]
pub struct SyntaxStyle;

/// A preview-driven editor scroll that is converging on a source block.
///
/// The editor exposes only a pixel offset, so the target line is found by
/// bracketing the monotonic offset-to-block mapping: `low` is a scroll
/// magnitude known to be above the block, `high` one known to be at or below
/// it. Each editor layout narrows the bracket.
#[cfg(editor_gpuikit)]
#[derive(Clone, Copy, Debug)]
pub struct PreviewScrollTarget {
    /// The preview block that must end up at the viewport top.
    pub block: usize,
    /// A scroll magnitude whose top block is before `block`.
    pub low: f32,
    /// A scroll magnitude whose top block is at or after `block`.
    pub high: Option<f32>,
    /// Layouts spent on this target, so a pathological document cannot loop.
    pub attempts: u8,
    /// The offset currently requested from the editor.
    pub requested: f32,
    /// The last offset whose top block was evaluated.
    pub observed: Option<f32>,
    /// Frames spent waiting for a request to be laid out.
    pub waited: u8,
}

/// One open Markdown document.
pub struct Document {
    /// Stable identity used by pending editor edits.
    pub id: u64,
    /// File backing this document, if any.
    pub path: Option<PathBuf>,
    /// Name shown in the tab bar.
    pub title: SharedString,
    /// The editor entity.
    pub editor: Entity<EditorState>,
    /// Whether the buffer changed since the last save.
    pub dirty: bool,
    /// Whether the editor rejects user edits; programmatic writes still work.
    pub readonly: bool,
    /// The text as last written to disk (or first loaded).
    pub saved_text: String,
    /// Cached caret position, refreshed when the editor notifies.
    pub cursor: (usize, usize),
    /// Cached character count, refreshed when the editor notifies.
    pub chars: usize,
    /// The scroll viewport this document is rendered in (filled at render time).
    pub scroll: Rc<RefCell<ScrollHandle>>,
    /// Key under which the scroll viewport stores `scroll`.
    pub scroll_id: ElementId,
    /// Digest of the last journaled buffer, so cursor-only changes stay cheap.
    pub content_digest: [u8; 32],
    /// The preview view rendered beside the source editor.
    #[cfg(editor_gpuikit)]
    pub preview: Entity<TextViewState>,
    /// Top-level Markdown blocks and their source positions.
    #[cfg(editor_gpuikit)]
    pub blocks: Vec<SourceBlock>,
    /// The preview's current top block, to avoid feedback loops.
    #[cfg(editor_gpuikit)]
    pub preview_top: usize,
    /// The editor scroll offset the preview was last synced from.
    #[cfg(editor_gpuikit)]
    pub preview_source_offset: f32,
    /// A preview-driven editor scroll is in flight; its first applied offset
    /// must not be fed back into the preview.
    #[cfg(editor_gpuikit)]
    pub pending_editor_scroll: bool,
    /// The block a preview-driven scroll is converging on, if any.
    #[cfg(editor_gpuikit)]
    pub preview_scroll_target: Option<PreviewScrollTarget>,
    /// Measured display rows per source line, learned from successful syncs.
    #[cfg(editor_gpuikit)]
    pub preview_rows_per_line: f32,
    /// The preview's scrollbar was being dragged on the previous notification.
    #[cfg(editor_gpuikit)]
    pub preview_dragging: bool,
}

/// The data needed to create a document.
pub struct DocumentInit {
    /// Stable identity used by pending editor edits.
    pub id: u64,
    /// Name shown in the tab bar.
    pub title: SharedString,
    /// File backing the document, if any.
    pub path: Option<PathBuf>,
    /// Initial source.
    pub text: String,
    /// Whether to start in raw-Markdown mode.
    pub raw: bool,
}

/// A persistent highlight over a marked editor range.
///
/// The handle owns one range-decoration collection. Dropping it leaves the
/// decoration in place; call [`Self::clear`] when the agent edit is done.
pub struct RangeHighlight {
    #[cfg(editor_gpuikit)]
    collection: gpui_kit::component::input::RangeDecorationCollection,
}

impl RangeHighlight {
    /// Remove the highlight from its editor.
    pub fn clear(&self, cx: &mut App) {
        #[cfg(editor_gpuikit)]
        self.collection.clear(cx);
        #[cfg(not(editor_gpuikit))]
        let _ = cx;
    }

    /// The tracked byte ranges, for tests.
    #[cfg(all(test, editor_gpuikit))]
    pub fn ranges(&self, cx: &App) -> Vec<Range<usize>> {
        self.collection.get_ranges(cx)
    }
}

/// Highlight a UTF-8 byte range until the returned handle is cleared.
///
/// The gpuikit backend paints a selection-coloured fill with a primary frame;
/// the zorite backend has no equivalent and returns an inert handle.
pub fn mark_editor_range(
    editor: &Entity<EditorState>,
    range: Range<usize>,
    fill: Hsla,
    frame: Hsla,
    cx: &mut App,
) -> RangeHighlight {
    #[cfg(editor_gpuikit)]
    {
        use gpui_kit::component::input::{RangeDecoration, RangeDecorationStyle};

        let collection = editor.update(cx, |editor, cx| {
            editor.create_range_decorations_collection(
                vec![
                    RangeDecoration::new(range.clone())
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(fill),
                    RangeDecoration::new(range)
                        .with_style(RangeDecorationStyle::Frame)
                        .with_color(frame),
                ],
                cx,
            )
        });
        RangeHighlight { collection }
    }
    #[cfg(not(editor_gpuikit))]
    {
        let _ = (editor, range, fill, frame, cx);
        RangeHighlight {}
    }
}

/// Collapse the editor's native selection to a byte offset.
pub fn set_editor_cursor(editor: &Entity<EditorState>, offset: usize, cx: &mut App) {
    #[cfg(editor_gpuikit)]
    editor.update(cx, |editor, cx| {
        editor.set_selected_range(offset..offset, cx)
    });
    #[cfg(editor_zorite)]
    editor.update(cx, |editor, cx| editor.set_cursor(offset, cx));
}

/// Lock or unlock user edits in an editor.
///
/// gpuikit keeps programmatic replacements editable while read-only mode only
/// rejects what the user types, which is exactly what the agent needs.
pub fn set_editor_readonly(editor: &Entity<EditorState>, readonly: bool, cx: &mut App) {
    #[cfg(editor_gpuikit)]
    editor.update(cx, |editor, cx| editor.set_readonly(readonly, cx));
    #[cfg(not(editor_gpuikit))]
    let _ = (editor, readonly, cx);
}

/// One top-level Markdown block and where it starts in the source.
#[cfg(editor_gpuikit)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceBlock {
    /// Zero-based source line.
    pub start_line: usize,
    /// Byte offset of [`Self::start_line`].
    pub start_byte: usize,
}

/// Split the source into the top-level blocks the preview renders.
///
/// The preview parser and comrak disagree only in rare edge cases; both treat
/// a heading, paragraph, list, table, and fenced block as one top-level block.
#[cfg(editor_gpuikit)]
pub fn source_blocks(text: &str) -> Vec<SourceBlock> {
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;

    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, text, &options);
    let mut blocks = Vec::new();
    let mut line_start = 0usize;
    let mut line = 0usize;

    for node in root.children() {
        let start_line = node.data.borrow().sourcepos.start.line.saturating_sub(1);
        while line < start_line {
            line_start += text[line_start..]
                .find('\n')
                .map_or(text.len() - line_start, |end| end + 1);
            line += 1;
        }
        blocks.push(SourceBlock {
            start_line,
            start_byte: line_start.min(text.len()),
        });
    }
    blocks
}

impl Document {
    /// Create a document with the given editor style.
    pub fn new(window: &mut Window, cx: &mut App, style: &SyntaxStyle, init: DocumentInit) -> Self {
        let content_digest = *blake3::hash(init.text.as_bytes()).as_bytes();
        let scroll_id = ElementId::Name(format!("editor-scroll-{}", init.id).into());
        #[cfg(editor_gpuikit)]
        let preview_text = init.text.clone();

        #[cfg(editor_zorite)]
        let editor = {
            let base = init
                .path
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            let images = image_provider(base);
            cx.new(|cx| {
                let mut editor = EditorState::new(window, cx)
                    .with_placeholder("Write your paper…")
                    .with_text(init.text);
                editor.set_tab_indent(4);
                editor.set_code_languages(
                    [
                        "text",
                        "rust",
                        "python",
                        "r",
                        "julia",
                        "javascript",
                        "bash",
                        "json",
                        "yaml",
                        "toml",
                        "markdown",
                    ]
                    .into_iter()
                    .map(SharedString::from)
                    .collect(),
                );
                editor.set_block_image_provider(images);
                editor.set_block_chip_provider(|src| {
                    let lower = src.to_ascii_lowercase();
                    [".pdf", ".epub", ".docx", ".html", ".htm", ".txt"]
                        .iter()
                        .any(|extension| lower.ends_with(extension))
                        .then(|| {
                            SharedString::from(src.rsplit('/').next().unwrap_or(src).to_owned())
                        })
                });
                if !init.raw {
                    editor.set_markdown_style(style.clone(), cx);
                }
                editor
            })
        };

        #[cfg(editor_gpuikit)]
        let editor = {
            let _ = (style, init.raw);
            let language = crate::highlight::language_for_path(init.path.as_deref());
            cx.new(|cx| {
                let mut editor = EditorState::new(window, cx)
                    .language(language)
                    .line_number(false)
                    .soft_wrap(true)
                    .default_value(init.text)
                    .placeholder("Write your paper…");
                let factory: gpui_kit::base::input::InputHighlighterFactory =
                    Rc::new(crate::highlight::highlighter_for);
                editor.set_highlighter_factory(factory, cx);
                editor
            })
        };

        let (saved_text, cursor, chars) = {
            let editor = editor.read(cx);
            #[cfg(editor_zorite)]
            let text = editor.text().to_owned();
            #[cfg(editor_gpuikit)]
            let text = editor.value().to_string();
            let cursor = line_column(&text, editor.cursor());
            let chars = text.chars().count();
            (text, cursor, chars)
        };
        #[cfg(editor_gpuikit)]
        let preview = cx.new(|cx| TextViewState::markdown(&preview_text, cx));
        #[cfg(editor_gpuikit)]
        let blocks = source_blocks(&preview_text);
        Self {
            id: init.id,
            path: init.path,
            title: init.title,
            editor,
            dirty: false,
            readonly: false,
            saved_text,
            cursor,
            chars,
            scroll: Rc::new(RefCell::new(ScrollHandle::new())),
            scroll_id,
            content_digest,
            #[cfg(editor_gpuikit)]
            preview,
            #[cfg(editor_gpuikit)]
            blocks,
            #[cfg(editor_gpuikit)]
            preview_top: 0,
            #[cfg(editor_gpuikit)]
            preview_source_offset: 0.0,
            #[cfg(editor_gpuikit)]
            pending_editor_scroll: false,
            #[cfg(editor_gpuikit)]
            preview_scroll_target: None,
            #[cfg(editor_gpuikit)]
            preview_rows_per_line: 1.0,
            #[cfg(editor_gpuikit)]
            preview_dragging: false,
        }
    }

    /// The current source.
    pub fn text(&self, cx: &App) -> String {
        #[cfg(editor_zorite)]
        {
            self.editor.read(cx).text().to_owned()
        }
        #[cfg(editor_gpuikit)]
        {
            self.editor.read(cx).value().to_string()
        }
    }

    /// Send new source to the preview and rebuild the block map.
    #[cfg(editor_gpuikit)]
    pub fn refresh_preview(&mut self, text: &str, cx: &mut App) {
        self.blocks = source_blocks(text);
        self.preview
            .update(cx, |preview, cx| preview.set_text(text, cx));
    }

    /// The top-level block that contains `byte`, clamped to the block list.
    #[cfg(editor_gpuikit)]
    pub fn block_for_byte(&self, byte: usize) -> usize {
        self.blocks
            .partition_point(|block| block.start_byte <= byte)
            .saturating_sub(1)
            .min(self.blocks.len().saturating_sub(1))
    }

    /// The active selection as a byte range, or `None` when it is collapsed.
    fn selection_bytes(&self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        #[cfg(editor_zorite)]
        {
            let text = self.editor.read(cx).text().to_owned();
            self.editor
                .update(cx, |editor, cx| {
                    editor.selected_text_range(false, window, cx)
                })
                .map(|selection| {
                    let start = utf16_to_byte(&text, selection.range.start);
                    let end = utf16_to_byte(&text, selection.range.end);
                    if start <= end { start..end } else { end..start }
                })
                .filter(|range| !range.is_empty())
        }
        #[cfg(editor_gpuikit)]
        {
            let _ = window;
            let range = self.editor.read(cx).selected_range();
            (!range.is_empty()).then_some(range)
        }
    }

    /// The byte range of the current selection, or the caret when collapsed.
    pub fn edit_range(&self, window: &mut Window, cx: &mut App) -> Range<usize> {
        let cursor = self.editor.read(cx).cursor();
        self.selection_bytes(window, cx).unwrap_or(cursor..cursor)
    }

    /// The selection as a character range, when one is marked.
    pub fn selection_chars(&self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        let range = self.selection_bytes(window, cx)?;
        let text = self.text(cx);
        let start = byte_to_char_index(&text, range.start);
        let end = byte_to_char_index(&text, range.end);
        Some(start..end)
    }

    /// The caret's 0-based `(line, column)`.
    pub fn cursor_position(&self, cx: &App) -> (usize, usize) {
        let text = self.text(cx);
        line_column(&text, self.editor.read(cx).cursor())
    }

    /// Focus this document's editor.
    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle(cx)
    }

    /// Move the caret to a byte offset.
    pub fn set_cursor(&self, offset: usize, cx: &mut App) {
        #[cfg(editor_zorite)]
        self.editor
            .update(cx, |editor, cx| editor.set_cursor(offset, cx));
        #[cfg(editor_gpuikit)]
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(offset..offset, cx)
        });
    }

    /// Replace one byte range as a single undo step.
    pub fn replace(&self, range: Range<usize>, text: &str, window: &mut Window, cx: &mut App) {
        #[cfg(editor_zorite)]
        {
            let _ = window;
            self.editor
                .update(cx, |editor, cx| editor.replace_range(range, text, cx));
        }
        #[cfg(editor_gpuikit)]
        {
            self.editor.update(cx, |editor, cx| {
                editor.set_selected_range(range, cx);
                editor.replace(text, window, cx);
            });
        }
    }

    /// Insert `text` at the current selection or caret.
    pub fn insert(&self, text: &str, window: &mut Window, cx: &mut App) {
        let range = self.edit_range(window, cx);
        self.replace(range, text, window, cx);
    }

    /// Set the declarative read-only flag; the next render applies it.
    pub fn set_readonly(&mut self, readonly: bool) {
        self.readonly = readonly;
    }

    /// Whether saving must ask for a real file name first.
    ///
    /// True for a buffer that has never been written and for the
    /// `Untitled-<n>.md` placeholder a new workspace scaffolds.
    pub fn needs_name(&self) -> bool {
        self.path.as_deref().is_none_or(is_placeholder_path)
    }

    /// The name a Save As dialog should propose.
    pub fn suggested_save_name(&self) -> String {
        suggested_save_name(self.path.as_deref(), &self.title)
    }

    /// Save to the existing path.
    pub fn save(&mut self, cx: &App) -> Result<(), String> {
        if self.needs_name() {
            return Err("this document still has a placeholder name; use Save as…".to_owned());
        }
        let Some(path) = self.path.clone() else {
            return Err("this document has no path yet; use Save as…".to_owned());
        };
        let text = self.text(cx);
        fs::write(&path, &text)
            .map_err(|error| format!("cannot write '{}': {error}", path.display()))?;
        self.saved_text = text;
        self.dirty = false;
        self.title = title_for(&path);
        Ok(())
    }

    /// Save to `path` and adopt it.
    pub fn save_as(&mut self, path: PathBuf, cx: &App) -> Result<(), String> {
        let text = self.text(cx);
        fs::write(&path, &text)
            .map_err(|error| format!("cannot write '{}': {error}", path.display()))?;
        self.path = Some(path.clone());
        self.title = title_for(&path);
        self.saved_text = text;
        self.dirty = false;
        Ok(())
    }
}

impl EditorSurface for Document {
    fn id(&self) -> u64 {
        self.id
    }

    fn name(&self) -> SharedString {
        self.title.clone()
    }

    fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn text(&self, cx: &App) -> SharedString {
        Document::text(self, cx).into()
    }

    fn cursor(&self, cx: &App) -> (usize, usize) {
        self.cursor_position(cx)
    }

    fn selection(&self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.selection_chars(window, cx)
    }

    fn replace(&self, range: Range<usize>, text: &str, window: &mut Window, cx: &mut App) {
        Document::replace(self, range, text, window, cx);
    }

    fn focus(&self, window: &mut Window, cx: &mut App) {
        let handle = self.editor.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn reveal(&self, offset: usize, window: &mut Window, cx: &mut App) {
        let content = self.text(cx);
        let byte = char_range_to_bytes(&content, offset..offset).start;
        self.set_cursor(byte, cx);

        // Zorite leaves scrolling to the host. The caret bounds come from the
        // last paint and stay valid for a caret move that does not edit text.
        #[cfg(editor_zorite)]
        {
            let editor = self.editor.read(cx);
            if let Some(caret) = editor.caret_screen_bounds() {
                let scroll = self.scroll.borrow();
                let viewport = scroll.bounds();
                if viewport.size.height > gpui_kit::px(0.) {
                    let margin = gpui_kit::px(64.);
                    let mut target = f32::from(scroll.offset().y);
                    if caret.top() < viewport.top() + margin {
                        target += f32::from(viewport.top() + margin - caret.top());
                    } else if caret.bottom() > viewport.bottom() - margin {
                        target -= f32::from(caret.bottom() - (viewport.bottom() - margin));
                    }
                    let max = f32::from(scroll.max_offset().y);
                    scroll.set_offset(gpui_kit::point(
                        gpui_kit::px(0.),
                        gpui_kit::px(target.clamp(-max, 0.0)),
                    ));
                }
            }
        }

        window.focus(&self.editor.read(cx).focus_handle(cx), cx);
    }
}

/// The display title for a path.
pub fn title_for(path: &Path) -> SharedString {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
        .into()
}

/// Whether `name` is one of Scico's generated placeholder names.
///
/// Accepts the workspace's `Untitled-1.md`, the `Untitled-2-3.md` collision
/// variants made by [`AppState::new_document`](crate::state::AppState::new_document),
/// and the extensionless `Untitled-2` title older sessions stored. A `.md`
/// suffix is accepted when present; the check is case-sensitive.
pub fn is_placeholder_name(name: &str) -> bool {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    let Some(rest) = stem.strip_prefix("Untitled-") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Whether `path` carries a generated [`is_placeholder_name`] file name.
pub fn is_placeholder_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_placeholder_name)
}

/// The name a Save As dialog proposes for `title` at `path`.
fn suggested_save_name(path: Option<&Path>, title: &str) -> String {
    match path {
        Some(path) if !is_placeholder_path(path) => title.to_owned(),
        _ if is_placeholder_name(title) => "paper.md".to_owned(),
        _ => title.to_owned(),
    }
}

/// Read a Markdown document, accepting any UTF-8 text file.
pub fn read_document(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("cannot read '{}': {error}", path.display()))
}

/// Load and cache standalone `![](path)` images relative to the document.
///
/// The editor asks on every paint, so both hits and misses are cached; an
/// unreadable or remote source falls back to its raw Markdown line.
#[cfg(editor_zorite)]
fn image_provider(base: PathBuf) -> impl Fn(&str) -> Option<Arc<RenderImage>> + 'static {
    let cache: RefCell<HashMap<PathBuf, Option<Arc<RenderImage>>>> = RefCell::new(HashMap::new());
    move |src| {
        if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
            return None;
        }
        let path = if Path::new(src).is_absolute() {
            PathBuf::from(src)
        } else {
            base.join(src)
        };
        if let Some(cached) = cache.borrow().get(&path) {
            return cached.clone();
        }
        let loaded = image::open(&path).ok().map(|image| {
            let frame = Frame::new(image.into_rgba8());
            Arc::new(RenderImage::new([frame]))
        });
        cache.borrow_mut().insert(path, loaded.clone());
        loaded
    }
}

/// Resolve a preview's image URI relative to the document.
///
/// `TextView`'s default handling is URI-based; this resolver keeps local
/// Markdown images working and caches both hits and misses.
#[cfg(editor_gpuikit)]
pub fn preview_image_source(
    base: PathBuf,
) -> impl Fn(&gpui_kit::SharedUri) -> gpui_kit::ImageSource + Send + Sync + 'static {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    let cache: Arc<Mutex<HashMap<PathBuf, Option<Arc<RenderImage>>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    move |uri| {
        let src = uri.to_string();
        if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
            return gpui_kit::ImageSource::from(uri.clone());
        }
        let path = if Path::new(&src).is_absolute() {
            PathBuf::from(&src)
        } else {
            base.join(&src)
        };
        let mut cache = cache.lock().expect("preview image cache");
        if let Some(cached) = cache.get(&path) {
            return cached.clone().map_or_else(
                || gpui_kit::ImageSource::from(uri.clone()),
                gpui_kit::ImageSource::Render,
            );
        }
        let loaded = image::open(&path).ok().map(|image| {
            let frame = Frame::new(image.into_rgba8());
            Arc::new(RenderImage::new([frame]))
        });
        let source = loaded.clone().map_or_else(
            || gpui_kit::ImageSource::from(uri.clone()),
            gpui_kit::ImageSource::Render,
        );
        cache.insert(path, loaded);
        source
    }
}

/// The 0-based `(line, column)` of a byte offset.
pub fn line_column(text: &str, cursor: usize) -> (usize, usize) {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let before = &text[..cursor];
    let line = before.matches('\n').count();
    let column = before.rsplit('\n').next().unwrap_or("").chars().count();
    (line, column)
}

/// Convert a UTF-16 offset to a byte offset, clamped to a char boundary.
#[cfg(editor_zorite)]
pub fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0usize;
    for (byte, character) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += character.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(editor_zorite)]
    #[test]
    fn utf16_offsets_round_trip_for_multibyte_text() {
        assert_eq!(utf16_to_byte("abc", 0), 0);
        assert_eq!(utf16_to_byte("abc", 2), 2);
        assert_eq!(utf16_to_byte("a😀b", 1), 1);
        assert_eq!(utf16_to_byte("a😀b", 3), 5);
        assert_eq!(utf16_to_byte("a😀b", 99), 6);
    }

    #[test]
    fn titles_use_the_file_name() {
        assert_eq!(title_for(Path::new("/tmp/draft.md")), "draft.md");
    }

    #[test]
    fn line_columns_are_zero_based_and_multibyte_aware() {
        assert_eq!(line_column("abc", 0), (0, 0));
        assert_eq!(line_column("abc\ndef", 5), (1, 1));
        assert_eq!(line_column("é😀\nx", 7), (1, 0));
        assert_eq!(line_column("é😀\nx", 4), (0, 1));
        assert_eq!(line_column("abc", 99), (0, 3));
    }

    #[test]
    fn placeholder_names_are_recognised() {
        assert!(is_placeholder_name("Untitled-1.md"));
        assert!(is_placeholder_name("Untitled-12-3.md"));
        assert!(is_placeholder_name("Untitled-2"));
        assert!(!is_placeholder_name("Untitled-.md"));
        assert!(!is_placeholder_name("Untitled-1.markdown"));
        assert!(!is_placeholder_name("Untitled-1-notes.md"));
        assert!(!is_placeholder_name("notes.md"));
        assert!(!is_placeholder_name("MyUntitled-1.md"));
    }

    #[test]
    fn save_as_suggests_a_real_name_for_placeholders() {
        assert_eq!(suggested_save_name(None, "Untitled-1.md"), "paper.md");
        assert_eq!(suggested_save_name(None, "Chat.md"), "Chat.md");
        assert_eq!(
            suggested_save_name(Some(Path::new("/tmp/Untitled-1.md")), "Untitled-1.md"),
            "paper.md"
        );
        assert_eq!(
            suggested_save_name(Some(Path::new("/tmp/notes.md")), "notes.md"),
            "notes.md"
        );
    }
}
