//! Syntax highlighting independent of any editor backend.
//!
//! GPUI Kit's editor asks its host for an [`InputHighlighter`]. This module
//! answers with a Syntect-backed adapter: Syntect parses the buffer, and the
//! TextMate scopes are mapped onto the semantic style names that the
//! component theme already resolves. The palette therefore keeps following
//! the application theme, and adding a language only adds a grammar and a
//! few scope selectors here.
//!
//! Parsing is incremental. Every parsed line keeps the parser state after it;
//! an edit re-parses from the changed line and stops as soon as the state and
//! the remaining source agree with the previous parse. Normal typing therefore
//! costs one line of parsing, not one document.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use gpui_kit::base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope,
};
use gpui_kit::{Context, HighlightStyle, SharedString, Window};
use syntect::highlighting::ScopeSelector;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// One styled run with its semantic name, if the scope has one.
type Run = (Range<usize>, Option<&'static str>);

/// A parsed line and the state that follows it.
struct ParsedLine {
    /// Absolute byte offset of the line in [`SyntectHighlighter::source`].
    start: usize,
    /// Absolute byte offset just past the line, including its newline.
    end: usize,
    /// Runs within this line, as absolute byte offsets.
    runs: Vec<Run>,
    /// Parser state after this line.
    next_state: ParseState,
    /// Scope stack after this line.
    next_scopes: ScopeStack,
}

/// The Syntect grammar set, loaded once per process.
fn syntax_set() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The editor language for a document path.
///
/// Untitled buffers are Markdown, because that is what this application
/// writes. Known non-Markdown formats keep their own grammar; any other
/// extension stays unhighlighted rather than guessing.
pub fn language_for_path(path: Option<&Path>) -> &'static str {
    let Some(extension) = path.and_then(Path::extension).and_then(|ext| ext.to_str()) else {
        return "markdown";
    };
    match extension.to_ascii_lowercase().as_str() {
        "md" | "markdown" | "mdown" | "mkd" | "mdx" | "txt" => "markdown",
        "bib" | "bibtex" => "bibtex",
        "tex" | "latex" | "sty" | "cls" => "latex",
        _ => "text",
    }
}

/// Build the highlighter for a language name.
///
/// This is the function installed as the editor's
/// [`InputHighlighterFactory`](gpui_kit::base::input::InputHighlighterFactory).
pub fn highlighter_for(language: &str) -> Option<Box<dyn InputHighlighter>> {
    Some(Box::new(SyntectHighlighter::new(language)))
}

/// A SyntaxSet-backed [`InputHighlighter`].
pub struct SyntectHighlighter {
    language: SharedString,
    syntax: &'static SyntaxReference,
    /// The source the cached lines refer to.
    source: String,
    /// One record per line, in order.
    lines: Vec<ParsedLine>,
    /// Number of lines parsed by the last update (for tests).
    #[cfg(test)]
    last_parsed_lines: usize,
}

impl SyntectHighlighter {
    /// Create a highlighter for `language`, falling back to plain text.
    pub fn new(language: &str) -> Self {
        let syntaxes = syntax_set();
        let syntax = match language.to_ascii_lowercase().as_str() {
            "markdown" | "md" => syntaxes.find_syntax_by_name("Markdown"),
            "bibtex" | "bib" => syntaxes.find_syntax_by_name("BibTeX"),
            "latex" | "tex" => syntaxes.find_syntax_by_name("LaTeX"),
            other => syntaxes
                .find_syntax_by_name(other)
                .or_else(|| syntaxes.find_syntax_by_extension(other)),
        }
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text());

        Self {
            language: language.into(),
            syntax,
            source: String::new(),
            lines: Vec::new(),
            #[cfg(test)]
            last_parsed_lines: 0,
        }
    }

    /// Reparse after `edit`, reusing every line whose state is unchanged.
    fn reparse(&mut self, source: &str, edit: Option<InputEdit>) {
        let Some(edit) = edit.filter(|_| !self.lines.is_empty() && !self.source.is_empty()) else {
            self.reparse_all(source);
            return;
        };

        let syntaxes = syntax_set();
        let mut names = HashMap::<Vec<Scope>, Option<&'static str>>::new();

        // The edit starts on this line. Starting one line earlier covers an
        // edit that rewrites the previous line's terminator.
        let first_line = line_index_at(source, edit.start_byte).saturating_sub(1);
        let first_line = first_line.min(self.lines.len().saturating_sub(1));

        // Move the unchanged prefix into the new cache; clone one parser state
        // to continue from, and keep the old tail for convergence checks.
        let old_lines = std::mem::take(&mut self.lines);
        let mut old_lines = old_lines.into_iter();
        let mut lines: Vec<ParsedLine> = old_lines.by_ref().take(first_line).collect();
        let old_tail: Vec<ParsedLine> = old_lines.collect();
        let (mut state, mut scopes) = match lines.last() {
            Some(previous) => (previous.next_state.clone(), previous.next_scopes.clone()),
            None => (ParseState::new(self.syntax), ScopeStack::new()),
        };
        let mut line_start = lines.last().map_or(0, |line| line.end);
        let mut reused_from = None;
        #[cfg(test)]
        let mut last_index = None;

        for (index, line) in source[line_start..].split_inclusive('\n').enumerate() {
            let runs = parse_line(
                &mut state,
                &mut scopes,
                &mut names,
                line,
                line_start,
                syntaxes,
            );
            let line_end = line_start + line.len();
            let next_state = state.clone();
            let next_scopes = scopes.clone();
            #[cfg(test)]
            {
                last_index = Some(index);
            }

            if let Some(old) = old_tail.get(index)
                && old.next_state == next_state
                && old.next_scopes == next_scopes
                && source.len() >= line_end
                && self.source.len() >= old.end
                && source[line_end..] == self.source[old.end..]
            {
                reused_from = Some(index);
                lines.push(ParsedLine {
                    start: line_start,
                    end: line_end,
                    runs,
                    next_state,
                    next_scopes,
                });
                break;
            }

            lines.push(ParsedLine {
                start: line_start,
                end: line_end,
                runs,
                next_state,
                next_scopes,
            });
            line_start = line_end;
        }

        if let Some(index) = reused_from {
            let shift = source.len() as isize - self.source.len() as isize;
            lines.extend(
                old_tail
                    .into_iter()
                    .skip(index + 1)
                    .map(|line| shift_line(line, shift)),
            );
        }

        self.source = source.to_owned();
        self.lines = lines;
        #[cfg(test)]
        {
            self.last_parsed_lines = last_index.map_or(0, |index| index + 1);
        }
    }

    fn reparse_all(&mut self, source: &str) {
        let syntaxes = syntax_set();
        let mut state = ParseState::new(self.syntax);
        let mut scopes = ScopeStack::new();
        let mut names = HashMap::<Vec<Scope>, Option<&'static str>>::new();
        let mut lines = Vec::new();
        let mut line_start = 0usize;

        for line in source.split_inclusive('\n') {
            let runs = parse_line(
                &mut state,
                &mut scopes,
                &mut names,
                line,
                line_start,
                syntaxes,
            );
            let line_end = line_start + line.len();
            lines.push(ParsedLine {
                start: line_start,
                end: line_end,
                runs,
                next_state: state.clone(),
                next_scopes: scopes.clone(),
            });
            line_start = line_end;
        }

        self.source = source.to_owned();
        #[cfg(test)]
        {
            self.last_parsed_lines = lines.len();
        }
        self.lines = lines;
    }

    /// The styled runs, for tests and diagnostics.
    #[cfg(test)]
    fn run_names(&self) -> impl Iterator<Item = Option<&'static str>> {
        self.lines
            .iter()
            .flat_map(|line| line.runs.iter().map(|(_, name)| *name))
    }
}

impl InputHighlighter for SyntectHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.reparse(&text.to_string(), edit);
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        if range.is_empty() || self.lines.is_empty() {
            return Vec::new();
        }
        let start = range.start.min(self.source.len());
        let end = range.end.min(self.source.len()).max(start);
        if start >= end {
            return Vec::new();
        }

        let first = self.lines.partition_point(|line| line.end <= start);
        let mut styles = Vec::new();
        let mut cursor = start;

        for line in &self.lines[first..] {
            if line.start >= end {
                break;
            }
            for (run, name) in &line.runs {
                let run_start = run.start.max(start);
                let run_end = run.end.min(end);
                if run_end <= cursor {
                    continue;
                }
                if run_start > cursor {
                    push_style(&mut styles, cursor..run_start, HighlightStyle::default());
                }
                let style = name
                    .and_then(|name| resolver.style(name))
                    .unwrap_or_default();
                push_style(&mut styles, run_start..run_end, style);
                cursor = run_end;
            }
        }

        if cursor < end {
            push_style(&mut styles, cursor..end, HighlightStyle::default());
        }
        styles
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

/// Parse one line, applying its scope operations to `state`/`scopes`.
fn parse_line(
    state: &mut ParseState,
    scopes: &mut ScopeStack,
    names: &mut HashMap<Vec<Scope>, Option<&'static str>>,
    line: &str,
    line_start: usize,
    syntaxes: &SyntaxSet,
) -> Vec<Run> {
    // Syntect's newline-aware grammars expect the terminator. Parse a synthetic
    // one for a final line that lacks it, but only emit runs for the bytes that
    // are actually in the buffer.
    let mut parsed = line.to_owned();
    if !parsed.ends_with('\n') {
        parsed.push('\n');
    }
    let operations = state.parse_line(&parsed, syntaxes).unwrap_or_default();
    let mut runs = Vec::new();
    let mut cursor = 0usize;

    for (offset, operation) in operations {
        let offset = offset.min(line.len()).max(cursor);
        if offset > cursor {
            push_run(
                &mut runs,
                line_start + cursor,
                line_start + offset,
                semantic_name(scopes, names),
            );
        }
        let _ = scopes.apply(&operation);
        cursor = offset;
    }

    if cursor < line.len() {
        push_run(
            &mut runs,
            line_start + cursor,
            line_start + line.len(),
            semantic_name(scopes, names),
        );
    }
    runs
}

/// Shift a cached line when an edit changed the source length.
fn shift_line(line: ParsedLine, shift: isize) -> ParsedLine {
    let shift_offset = |offset: usize| (offset as isize + shift) as usize;
    ParsedLine {
        start: shift_offset(line.start),
        end: shift_offset(line.end),
        runs: line
            .runs
            .into_iter()
            .map(|(range, name)| (shift_offset(range.start)..shift_offset(range.end), name))
            .collect(),
        next_state: line.next_state,
        next_scopes: line.next_scopes,
    }
}

fn push_run(runs: &mut Vec<Run>, start: usize, end: usize, name: Option<&'static str>) {
    if start >= end {
        return;
    }
    if let Some((last, last_name)) = runs.last_mut()
        && last.end == start
        && *last_name == name
    {
        last.end = end;
        return;
    }
    runs.push((start..end, name));
}

fn push_style(
    styles: &mut Vec<(Range<usize>, HighlightStyle)>,
    range: Range<usize>,
    style: HighlightStyle,
) {
    if range.is_empty() {
        return;
    }
    if let Some((last, last_style)) = styles.last_mut()
        && last.end == range.start
        && *last_style == style
    {
        last.end = range.end;
        return;
    }
    styles.push((range, style));
}

/// The index of the line containing `offset`.
fn line_index_at(source: &str, offset: usize) -> usize {
    let offset = offset.min(source.len());
    source.as_bytes()[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

/// The best semantic name for a scope stack.
fn semantic_name(
    scopes: &ScopeStack,
    cache: &mut HashMap<Vec<Scope>, Option<&'static str>>,
) -> Option<&'static str> {
    if let Some(name) = cache.get(scopes.as_slice()) {
        return *name;
    }
    let name = selectors()
        .iter()
        .filter_map(|(selector, name)| {
            selector
                .does_match(scopes.as_slice())
                .map(|power| (power, *name))
        })
        .max_by_key(|(power, _)| *power)
        .map(|(_, name)| name);
    cache.insert(scopes.as_slice().to_vec(), name);
    name
}

/// Scope selectors in the semantic vocabulary of the component highlight
/// theme. More specific selectors win by [`syntect`]'s match power.
fn selectors() -> &'static [(ScopeSelector, &'static str)] {
    static SELECTORS: OnceLock<Vec<(ScopeSelector, &'static str)>> = OnceLock::new();
    SELECTORS.get_or_init(|| {
        [
            ("comment.block.documentation", "comment.doc"),
            ("comment.line.documentation", "comment.doc"),
            ("comment", "comment"),
            ("string.regexp", "string.regex"),
            ("constant.character.escape", "string.escape"),
            ("string", "string"),
            ("constant.numeric", "number"),
            ("constant.language", "constant"),
            ("constant", "constant"),
            ("support.constant", "constant"),
            ("keyword.control.import", "preproc"),
            ("meta.preprocessor", "preproc"),
            ("keyword", "keyword"),
            ("storage", "keyword"),
            ("support.function", "function"),
            ("entity.name.function", "function"),
            ("variable.function", "function"),
            ("support.type", "type"),
            ("support.class", "type"),
            ("entity.name.type", "type"),
            ("entity.name.class", "type"),
            ("entity.name.tag", "tag"),
            ("entity.other.attribute-name", "attribute"),
            ("variable.language", "variable.special"),
            ("variable", "variable"),
            ("punctuation.definition.list", "punctuation.list_marker"),
            ("punctuation.section", "punctuation.special"),
            ("punctuation", "punctuation"),
            ("markup.heading", "title"),
            ("markup.bold", "emphasis.strong"),
            ("markup.italic", "emphasis"),
            ("markup.underline.link", "link_uri"),
            ("string.other.link.title", "link_text"),
            ("markup.raw", "text.code.span"),
            ("markup.inline.raw", "text.code.span"),
            ("markup.quote", "comment"),
            ("support.type.property-name", "property"),
            ("meta.object-literal.key", "property"),
            ("entity.name.label", "label"),
            ("keyword.operator", "operator"),
            ("operator", "operator"),
        ]
        .into_iter()
        .filter_map(|(selector, name)| {
            selector
                .parse::<ScopeSelector>()
                .ok()
                .map(|selector| (selector, name))
        })
        .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestResolver;

    impl HighlightStyleResolver for TestResolver {
        fn style(&self, _name: &str) -> Option<HighlightStyle> {
            Some(HighlightStyle::default())
        }
    }

    fn build(language: &str, text: &str) -> SyntectHighlighter {
        let mut highlighter = SyntectHighlighter::new(language);
        highlighter.reparse(text, None);
        highlighter
    }

    fn edit(start: usize, old_end: usize, new_end: usize) -> InputEdit {
        InputEdit {
            start_byte: start,
            old_end_byte: old_end,
            new_end_byte: new_end,
            start_position: gpui_kit::base::input::Point::new(0, 0),
            old_end_position: gpui_kit::base::input::Point::new(0, 0),
            new_end_position: gpui_kit::base::input::Point::new(0, 0),
        }
    }

    #[test]
    fn languages_follow_the_extension() {
        assert_eq!(language_for_path(None), "markdown");
        assert_eq!(
            language_for_path(Some(Path::new("/tmp/paper.md"))),
            "markdown"
        );
        assert_eq!(
            language_for_path(Some(Path::new("/tmp/references.BIB"))),
            "bibtex"
        );
        assert_eq!(language_for_path(Some(Path::new("/tmp/main.tex"))), "latex");
        assert_eq!(language_for_path(Some(Path::new("/tmp/notes.rs"))), "text");
    }

    #[test]
    fn markdown_uses_semantic_runs_and_covers_the_buffer() {
        let text = "# Heading\n\nUse `code` and **bold**.\n";
        let highlighter = build("markdown", text);
        let names: Vec<_> = highlighter.run_names().collect();
        assert!(names.contains(&Some("title")), "{names:?}");
        assert!(names.contains(&Some("text.code.span")), "{names:?}");
        assert!(names.contains(&Some("emphasis.strong")), "{names:?}");

        let styles = highlighter.styles(&(0..text.len()), &TestResolver);
        assert_eq!(styles.first().expect("runs").0.start, 0);
        assert_eq!(styles.last().expect("runs").0.end, text.len());
        for pair in styles.windows(2) {
            assert_eq!(pair[0].0.end, pair[1].0.start, "runs must tile");
        }
    }

    #[test]
    fn bibtex_is_highlighted() {
        let text = "@article{kano2025,\n  title = {A Title},\n  year = {2025},\n}\n";
        let highlighter = build("bibtex", text);
        let names: Vec<_> = highlighter.run_names().flatten().collect();
        assert!(!names.is_empty(), "a BibTeX entry must style something");

        let styles = highlighter.styles(&(0..text.len()), &TestResolver);
        let covered: usize = styles.iter().map(|(range, _)| range.len()).sum();
        assert_eq!(covered, text.len());
    }

    #[test]
    fn styles_are_clipped_to_the_requested_range() {
        let text = "# Heading\n\nbody\n";
        let highlighter = build("markdown", text);
        let styles = highlighter.styles(&(2..12), &TestResolver);
        assert_eq!(styles.first().expect("runs").0.start, 2);
        assert_eq!(styles.last().expect("runs").0.end, 12);
        let covered: usize = styles.iter().map(|(range, _)| range.len()).sum();
        assert_eq!(covered, 10);
    }

    #[test]
    fn an_edit_reuses_the_unchanged_tail() {
        let text = (0..300)
            .map(|line| format!("Paragraph {line}.\n"))
            .collect::<String>();
        let mut highlighter = build("markdown", &text);
        assert_eq!(highlighter.last_parsed_lines, 300);

        // Replace "Paragraph 1." with "Paragraph one." in the second line.
        let start = text.find("Paragraph 1.").expect("line");
        let mut edited = text.clone();
        edited.replace_range(start..start + "Paragraph 1.".len(), "Paragraph one.");
        highlighter.reparse(
            &edited,
            Some(edit(
                start,
                start + "Paragraph 1.".len(),
                start + "Paragraph one.".len(),
            )),
        );
        assert!(
            highlighter.last_parsed_lines <= 3,
            "only the changed line should be parsed, parsed {}",
            highlighter.last_parsed_lines
        );

        let fresh = build("markdown", &edited);
        assert_eq!(
            highlighter.run_names().collect::<Vec<_>>(),
            fresh.run_names().collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_edit_that_opens_a_fence_reparses_the_tail() {
        let text = (0..50)
            .map(|line| format!("Paragraph {line}.\n"))
            .collect::<String>();
        let mut highlighter = build("markdown", &text);
        let start = text.find("Paragraph 10.").expect("line");
        let mut edited = text.clone();
        edited.replace_range(start..start, "```\n");
        let new_end = start + "```\n".len();
        highlighter.reparse(&edited, Some(edit(start, start, new_end)));

        let fresh = build("markdown", &edited);
        assert_eq!(
            highlighter.run_names().collect::<Vec<_>>(),
            fresh.run_names().collect::<Vec<_>>(),
            "an open fence changes the following lines"
        );
    }

    #[test]
    fn editing_a_large_buffer_parses_only_the_changed_neighborhood() {
        let block = "## A section\n\nA paragraph with `code`, **bold**, and a [link](x).\n\n";
        let text = block.repeat(1_000);
        let mut highlighter = build("markdown", &text);

        let mut edited = text.clone();
        let at = text.len() / 2;
        edited.insert(at, 'x');
        highlighter.reparse(&edited, Some(edit(at, at, at + 1)));

        assert!(
            highlighter.last_parsed_lines <= 3,
            "an in-line edit must not reparse the document, parsed {}",
            highlighter.last_parsed_lines
        );
        let fresh = build("markdown", &edited);
        assert_eq!(
            highlighter.run_names().collect::<Vec<_>>(),
            fresh.run_names().collect::<Vec<_>>()
        );
    }
}
