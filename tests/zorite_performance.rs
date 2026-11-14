//! Zorite performance regression guards.
//!
//! Scico was once moved off zorite because it was too slow, so the backend
//! keeps a timing guard. A paper-sized document (30 sections, about 22 KB) is
//! opened, typed into and switched to raw mode; the budgets are roughly five
//! times the debug-build cost measured on a developer machine. They catch an
//! order-of-magnitude regression (the kind that made us switch), not small
//! drift. Run with `--nocapture` to see the measured times.
//!
//! The large-document test is ignored because a full redraw there is quadratic
//! in the document size. Run it deliberately when touching paint or markdown
//! parsing:
//!
//! ```text
//! cargo test --no-default-features --features zorite \
//!     --test zorite_performance -- --ignored --nocapture
//! ```
#![cfg(editor_zorite)]

use std::time::{Duration, Instant};

use gpui_kit::{
    Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};
use scico::document::{Document, DocumentInit, EditorState};
use scico::theme::syntax_style;

/// A document about the size of a scientific paper.
const PAPER_SECTIONS: usize = 30;
/// Keystrokes measured per typing test.
const TYPED_CHARS: usize = 10;

const OPEN_BUDGET: Duration = Duration::from_millis(1500);
const TYPING_BUDGET: Duration = Duration::from_millis(2500);
const TOGGLE_BUDGET: Duration = Duration::from_millis(1500);
const LARGE_DRAW_BUDGET: Duration = Duration::from_secs(30);

struct Root {
    editor: Entity<EditorState>,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.editor.clone())
    }
}

fn document(sections: usize) -> String {
    let body = "word ".repeat(120);
    (0..sections)
        .map(|index| format!("## Section {index}\n\nParagraph {index}. {body}\n\n"))
        .collect()
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn mount(cx: &mut TestAppContext, text: String) -> (Entity<Root>, &mut VisualTestContext) {
    cx.add_window_view(|window, cx| {
        let style = syntax_style(cx);
        let document = Document::new(
            window,
            cx,
            &style,
            DocumentInit {
                id: 1,
                title: "perf.md".into(),
                path: None,
                text,
                raw: false,
            },
        );
        Root {
            editor: document.editor.clone(),
        }
    })
}

fn editor_of(root: &Entity<Root>, cx: &VisualTestContext) -> Entity<EditorState> {
    root.read_with(cx, |root, _| root.editor.clone())
}

#[gpui_kit::test]
fn opening_a_paper_sized_document_stays_within_budget(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let text = document(PAPER_SECTIONS);
    let (root, cx) = mount(cx, text.clone());
    let editor = editor_of(&root, cx);
    draw(cx);

    let started = Instant::now();
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| editor.set_text(text.clone(), cx));
    });
    draw(cx);
    let elapsed = started.elapsed();
    eprintln!("zorite: open {} bytes in {elapsed:?}", text.len());

    assert!(
        elapsed < OPEN_BUDGET,
        "opening a paper-sized document took {elapsed:?} (budget {OPEN_BUDGET:?})"
    );
}

#[gpui_kit::test]
fn typing_a_paper_sized_document_stays_within_budget(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let text = document(PAPER_SECTIONS);
    let (root, cx) = mount(cx, text.clone());
    let editor = editor_of(&root, cx);
    draw(cx);

    let started = Instant::now();
    for offset in (text.len() / 2..).take(TYPED_CHARS) {
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.replace_range(offset..offset, "x", cx)
            });
        });
        draw(cx);
    }
    let elapsed = started.elapsed();
    eprintln!("zorite: {TYPED_CHARS} keystrokes in {elapsed:?}");

    assert!(
        elapsed < TYPING_BUDGET,
        "{TYPED_CHARS} keystrokes took {elapsed:?} (budget {TYPING_BUDGET:?})"
    );
}

#[gpui_kit::test]
fn toggling_raw_mode_stays_within_budget(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let text = document(PAPER_SECTIONS);
    let (root, cx) = mount(cx, text);
    let editor = editor_of(&root, cx);
    draw(cx);

    let style = cx.update(|_, cx| syntax_style(cx));
    let started = Instant::now();
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| editor.clear_markdown_style(cx));
    });
    draw(cx);
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| editor.set_markdown_style(style, cx));
    });
    draw(cx);
    let elapsed = started.elapsed();
    eprintln!("zorite: raw-mode round trip in {elapsed:?}");

    assert!(
        elapsed < TOGGLE_BUDGET,
        "the raw-mode round trip took {elapsed:?} (budget {TOGGLE_BUDGET:?})"
    );
}

/// A full redraw of a large document. Zorite shapes a band per frame, so the
/// guard is deliberately loose; it fails when a change makes the whole-document
/// path visibly worse than the quadratic baseline.
#[gpui_kit::test]
#[ignore = "slow: run with --ignored to measure large-document scaling"]
fn a_large_document_redraw_stays_within_budget(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let text = document(300);
    let (root, cx) = mount(cx, text.clone());
    let editor = editor_of(&root, cx);
    draw(cx);

    let started = Instant::now();
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| editor.set_text(text.clone(), cx));
    });
    draw(cx);
    let elapsed = started.elapsed();
    eprintln!("zorite: redraw {} bytes in {elapsed:?}", text.len());

    assert!(
        elapsed < LARGE_DRAW_BUDGET,
        "redrawing a large document took {elapsed:?} (budget {LARGE_DRAW_BUDGET:?})"
    );
}
