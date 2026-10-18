//! Documents, tabs, and file operations.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{
    App, AppContext as _, Entity, EntityInputHandler as _, RenderImage, SharedString, Window,
};
use image::Frame;
use zorite_editor::{EditorState, SyntaxStyle};

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
        let base = init
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let images = image_provider(base);
        let editor = cx.new(|cx| {
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
                    .then(|| SharedString::from(src.rsplit('/').next().unwrap_or(src).to_owned()))
            });
            if !init.raw {
                editor.set_markdown_style(style.clone(), cx);
            }
            editor
        });
        let (saved_text, cursor, chars) = {
            let editor = editor.read(cx);
            let text = editor.text().to_owned();
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
        }
    }

    /// The current source.
    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).text().to_owned()
    }

    /// The byte range of the current selection, or the caret when collapsed.
    pub fn edit_range(&self, window: &mut Window, cx: &mut App) -> Range<usize> {
        let text = self.editor.read(cx).text().to_owned();
        let cursor = self.editor.read(cx).cursor();
        let selected = self
            .editor
            .update(cx, |editor, cx| {
                editor.selected_text_range(false, window, cx)
            })
            .map(|selection| {
                let start = utf16_to_byte(&text, selection.range.start);
                let end = utf16_to_byte(&text, selection.range.end);
                if start <= end { start..end } else { end..start }
            })
            .unwrap_or(cursor..cursor);
        if selected.is_empty() {
            cursor..cursor
        } else {
            selected
        }
    }

    /// The selection as a character range, when one is marked.
    pub fn selection_chars(&self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        let range = self.edit_range(window, cx);
        if range.is_empty() {
            return None;
        }
        let text = self.editor.read(cx).text().to_owned();
        let start = crate::roundtrip::byte_to_char_index(&text, range.start);
        let end = crate::roundtrip::byte_to_char_index(&text, range.end);
        Some(start..end)
    }

    /// The caret's 0-based `(line, column)`.
    pub fn cursor_position(&self, cx: &App) -> (usize, usize) {
        let text = self.editor.read(cx).text();
        line_column(text, self.editor.read(cx).cursor())
    }

    /// Replace one byte range as a single undo step.
    pub fn replace(&self, range: Range<usize>, text: &str, cx: &mut App) {
        self.editor
            .update(cx, |editor, cx| editor.replace_range(range, text, cx));
    }

    /// Insert `text` at the current selection or caret.
    pub fn insert(&self, text: &str, window: &mut Window, cx: &mut App) {
        let range = self.edit_range(window, cx);
        self.replace(range, text, cx);
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
