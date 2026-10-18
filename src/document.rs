//! Documents, tabs, and file operations.

use std::cell::RefCell;
#[cfg(feature = "zorite")]
use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
#[cfg(feature = "zorite")]
use std::sync::Arc;

#[cfg(feature = "zorite")]
use gpui_kit::EntityInputHandler as _;
#[cfg(any(feature = "zorite", feature = "gpuikit"))]
use gpui_kit::RenderImage;
use gpui_kit::{
    App, AppContext as _, ElementId, Entity, FocusHandle, Focusable as _, ScrollHandle,
    SharedString, Window,
};
#[cfg(any(feature = "zorite", feature = "gpuikit"))]
use image::Frame;
use ragrig_gpui::editor::{EditorSurface, byte_to_char_index};

#[cfg(feature = "gpuikit")]
pub use gpui_kit::component::input::EditorState;
#[cfg(feature = "zorite")]
pub use zorite_editor::{EditorState, SyntaxStyle};
#[cfg(feature = "gpuikit")]
/// Placeholder for zorite's theme type; the gpuikit editor follows the app theme.
#[derive(Clone, Debug, Default)]
pub struct SyntaxStyle;

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

impl Document {
    /// Create a document with the given editor style.
    pub fn new(window: &mut Window, cx: &mut App, style: &SyntaxStyle, init: DocumentInit) -> Self {
        let content_digest = *blake3::hash(init.text.as_bytes()).as_bytes();
        let scroll_id = ElementId::Name(format!("editor-scroll-{}", init.id).into());

        #[cfg(feature = "zorite")]
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

        #[cfg(feature = "gpuikit")]
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
            #[cfg(feature = "zorite")]
            let text = editor.text().to_owned();
            #[cfg(feature = "gpuikit")]
            let text = editor.value().to_string();
            let cursor = line_column(&text, editor.cursor());
            let chars = text.chars().count();
            (text, cursor, chars)
        };
        Self {
            id: init.id,
            path: init.path,
            title: init.title,
            editor,
            dirty: false,
            saved_text,
            cursor,
            chars,
            scroll: Rc::new(RefCell::new(ScrollHandle::new())),
            scroll_id,
            content_digest,
        }
    }

    /// The current source.
    pub fn text(&self, cx: &App) -> String {
        #[cfg(feature = "zorite")]
        {
            self.editor.read(cx).text().to_owned()
        }
        #[cfg(feature = "gpuikit")]
        {
            self.editor.read(cx).value().to_string()
        }
    }

    /// The active selection as a byte range, or `None` when it is collapsed.
    fn selection_bytes(&self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        #[cfg(feature = "zorite")]
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
        #[cfg(feature = "gpuikit")]
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
        #[cfg(feature = "zorite")]
        self.editor
            .update(cx, |editor, cx| editor.set_cursor(offset, cx));
        #[cfg(feature = "gpuikit")]
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(offset..offset, cx)
        });
    }

    /// Replace one byte range as a single undo step.
    pub fn replace(&self, range: Range<usize>, text: &str, window: &mut Window, cx: &mut App) {
        #[cfg(feature = "zorite")]
        {
            let _ = window;
            self.editor
                .update(cx, |editor, cx| editor.replace_range(range, text, cx));
        }
        #[cfg(feature = "gpuikit")]
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

    /// Save to the existing path.
    pub fn save(&mut self, cx: &App) -> Result<(), String> {
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
}

/// The display title for a path.
pub fn title_for(path: &Path) -> SharedString {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
        .into()
}

/// Read a Markdown document, accepting any UTF-8 text file.
pub fn read_document(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("cannot read '{}': {error}", path.display()))
}

/// Load and cache standalone `![](path)` images relative to the document.
///
/// The editor asks on every paint, so both hits and misses are cached; an
/// unreadable or remote source falls back to its raw Markdown line.
#[cfg(feature = "zorite")]
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
#[cfg(feature = "gpuikit")]
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
#[cfg(feature = "zorite")]
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

    #[cfg(feature = "zorite")]
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
}
