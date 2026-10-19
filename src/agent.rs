//! The Scico pane: transcript, composer, context chips, and roundtrip.
//!
//! This is the Sciscraper-specific composite built on Ragrig's primitives
//! instead of `RagChat`, because a question has to carry the editor's own
//! containers (document, bibliography, marked selections) and a finished answer
//! may carry a replacement block that belongs back in the document.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window,
    component::{ActiveTheme as _, h_flex, v_flex},
    div,
    prelude::FluentBuilder as _,
    px,
};
use ragrig_gpui::prelude::*;

use crate::state::{AppState, AppStateHost};

/// What the Scico pane tells its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentPaneEvent {
    /// A one-line status for the window's status bar.
    Status(SharedString),
    /// The user asked for the full settings surface.
    OpenSettings,
}

/// A marked passage waiting for the next question.
#[derive(Clone)]
struct PendingSelection {
    id: String,
    tab_id: u64,
    context: RoundtripContext,
}

/// A replacement the host could not apply automatically.
#[derive(Clone, Debug)]
struct ReplacementOffer {
    text: String,
    reason: String,
}

struct PendingTurn {
    request: RequestId,
    message: MessageId,
    tokens: usize,
    /// The passage an edit answer may replace. `None` means context only.
    target: Option<PendingSelection>,
}

/// A direct-edit answer waiting for the next render's window.
struct PendingWrite {
    target: ReplacementTarget,
    text: String,
    message: MessageId,
    empty_prose: bool,
}

/// The Sciscraper Scico pane.
pub struct AgentPane {
    state: Entity<AppState>,
    roundtrip: Roundtrip,
    /// A direct-edit answer waiting for the next render's window.
    pending_write: Option<PendingWrite>,
    chat: Entity<ChatView>,
    composer: Entity<Composer>,
    pending: Option<PendingTurn>,
    attachments: Vec<PendingAttachment>,
    attachment_counter: u64,
    selections: Vec<PendingSelection>,
    selection_counter: u64,
    /// Selection id that the next submitted prompt must edit directly.
    edit_target: Option<String>,
    /// Selection id whose text also drives retrieval for the next prompt.
    enrich_target: Option<String>,
    offer: Option<ReplacementOffer>,
    streaming: bool,
    status: SharedString,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl AgentPane {
    /// Create the transcript, composer, and event wiring.
    pub fn new(window: &mut Window, cx: &mut Context<Self>, state: Entity<AppState>) -> Self {
        let chat = cx.new(ChatView::new);
        let composer = cx.new(|cx| Composer::new(window, cx));
        let mut subscriptions = Vec::new();

        subscriptions.push(
            cx.subscribe(&composer, |this, _, event: &ComposerEvent, cx| {
                this.on_composer(event.clone(), cx);
            }),
        );
        subscriptions.push(cx.subscribe(&chat, |this, _, event: &ChatViewEvent, cx| {
            this.on_chat_event(event.clone(), cx);
        }));
        let controller = state.read(cx).controller.clone();
        subscriptions.push(cx.subscribe(&controller, |this, _, event: &RagEvent, cx| {
            this.on_rag_event(event.clone(), cx);
        }));
        subscriptions.push(cx.observe(&state, |_, _, cx| cx.notify()));

        let status = controller.read(cx).status_line();
        let roundtrip = Roundtrip::new(Rc::new(AppStateHost {
            state: state.clone(),
        }));
        let this = Self {
            state,
            roundtrip,
            pending_write: None,
            chat,
            composer,
            pending: None,
            attachments: Vec::new(),
            attachment_counter: 0,
            selections: Vec::new(),
            selection_counter: 0,
            edit_target: None,
            enrich_target: None,
            offer: None,
            streaming: false,
            status: status.into(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.chat.update(cx, |chat, cx| {
            chat.set_empty_state(
                EmptyState {
                    icon: gpui_kit::assets::IconName::Sparkles,
                    title: "Ask your library".into(),
                    description: "Index a corpus in the Library, attach your draft, \
                                  then ask a question. The document and the marked \
                                  selections ride along as context."
                        .into(),
                },
                cx,
            );
        });
        this
    }

    /// Mark the active editor selection as context for the next question.
    ///
    /// Repeated marks accumulate, so one question can carry several passages.
    /// An identical range is not added twice. The captured container keeps the
    /// range and digest, so a later edit can only write back when the passage
    /// still matches.
    pub fn focus_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.capture_selection(window, cx) {
            Some((_, true)) => {
                let count = self.selections.len();
                let chars: usize = self
                    .selections
                    .iter()
                    .map(|selection| selection.context.chars())
                    .sum();
                self.set_status(
                    format!(
                        "{count} marked region(s) attached ({chars} chars) — ask, or use Edit/Enrich selection"
                    ),
                    cx,
                );
            }
            Some((_, false)) => {
                self.set_status("That passage is already marked", cx);
            }
            None => self.set_status("Mark a passage in the editor first", cx),
        }
    }

    /// Arm direct-edit mode for the marked selection.
    ///
    /// Uses the editor's current selection when there is one; otherwise the
    /// last marked passage. The next submitted prompt is written back to that
    /// passage instead of producing a chat answer with an Insert offer.
    pub fn edit_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self
            .capture_selection(window, cx)
            .map(|(id, _)| id)
            .or_else(|| self.selections.last().map(|selection| selection.id.clone()));
        let Some(id) = id else {
            self.set_status("Mark a passage in the editor first", cx);
            return;
        };
        self.edit_target = Some(id);
        self.enrich_target = None;
        self.set_status(
            "Edit mode — describe the change; the answer replaces the marked region",
            cx,
        );
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Arm enriched retrieval for the marked selection.
    ///
    /// The next prompt still replaces the marked passage, but the passage's
    /// text is prepended to the retrieval query, so the similarity search
    /// finds literature about the passage itself. This is the difference from
    /// [`Self::edit_selection`], which attaches the passage as ordinary
    /// context and retrieves from the instruction alone.
    pub fn enrich_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self
            .capture_selection(window, cx)
            .map(|(id, _)| id)
            .or_else(|| self.selections.last().map(|selection| selection.id.clone()));
        let Some(id) = id else {
            self.set_status("Mark a passage in the editor first", cx);
            return;
        };
        self.edit_target = Some(id.clone());
        self.enrich_target = Some(id);
        self.set_status(
            "Enrich mode — the marked region is the retrieval query; the answer replaces it",
            cx,
        );
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Capture the current selection, returning `(id, added)`.
    ///
    /// `added` is false when the same range is already in the list.
    fn capture_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(String, bool)> {
        let selection = self
            .roundtrip
            .selection_context(window, cx, MAX_CONTEXT_CHARS)?;
        let tab_id = selection.document_id;
        let context = selection.context;
        let range = context.range();
        let digest = context.digest().to_owned();
        if let Some(existing) = self.selections.iter().find(|selection| {
            selection.tab_id == tab_id
                && selection.context.range() == range
                && selection.context.digest() == digest
        }) {
            return Some((existing.id.clone(), false));
        }
        self.selection_counter = self.selection_counter.wrapping_add(1);
        let id = format!("selection-{}", self.selection_counter);
        self.selections.push(PendingSelection {
            id: id.clone(),
            tab_id,
            context,
        });
        Some((id, true))
    }

    fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
    }

    /// Insert text into the active document at the selection or caret.
    ///
    /// This is the Library → Editor direction and travels through the
    /// roundtrip contract, so the agent never names the editor backend.
    pub fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let inserted = self.roundtrip.insert(text, window, cx);
        if inserted {
            cx.notify();
        }
        inserted
    }

    /// Whether an answer is currently streaming.
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Characters in all marked passages waiting for the next question.
    pub fn pending_selection_chars(&self) -> Option<usize> {
        (!self.selections.is_empty()).then(|| {
            self.selections
                .iter()
                .map(|selection| selection.context.chars())
                .sum()
        })
    }

    /// How many passages are marked for the next question.
    pub fn pending_selection_count(&self) -> usize {
        self.selections.len()
    }

    /// Marked passages waiting for the next question as `(tab id, char range)`.
    ///
    /// The editor draws its marching-ant frame for the entries of the active
    /// tab; sending the prompt consumes them.
    pub fn marked_regions(&self) -> Vec<(u64, std::ops::Range<usize>)> {
        self.selections
            .iter()
            .filter_map(|selection| Some((selection.tab_id, selection.context.range()?)))
            .collect()
    }

    /// The latest Scico status line.
    pub fn status(&self) -> &SharedString {
        &self.status
    }

    /// The first pending selection's tab, char range, and digest, for write-back tests.
    #[cfg(test)]
    pub(crate) fn pending_selection_guard(&self) -> Option<(u64, std::ops::Range<usize>, String)> {
        self.selections.first().map(|selection| {
            (
                selection.tab_id,
                selection.context.range().unwrap_or(0..0),
                selection.context.digest().to_owned(),
            )
        })
    }

    /// Whether direct-edit mode is armed.
    #[cfg(test)]
    pub(crate) fn edit_target_is_set(&self) -> bool {
        self.edit_target.is_some()
    }

    /// Whether enriched retrieval is armed.
    #[cfg(test)]
    pub(crate) fn enrich_target_is_set(&self) -> bool {
        self.enrich_target.is_some()
    }

    /// Queue files as one-shot context for the next question.
    pub fn attach_files(
        &mut self,
        files: impl IntoIterator<Item = (String, PathBuf)>,
        cx: &mut Context<Self>,
    ) {
        let mut count = 0usize;
        for (name, path) in files {
            self.attachment_counter = self.attachment_counter.wrapping_add(1);
            let id = format!("file-{}", self.attachment_counter);
            self.attachments
                .push(PendingAttachment::file(id, name, path));
            count += 1;
        }
        if count > 0 {
            self.set_status(
                format!("Attached {count} paper(s) to the next question"),
                cx,
            );
        }
        cx.notify();
    }

    /// Stop the running answer, keeping the partial text.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.pending.as_ref().map(|pending| pending.request) else {
            return;
        };
        let controller = self.controller(cx);
        controller.update(cx, |controller, cx| controller.stop(request, cx));
        self.set_status("Stopping…", cx);
    }

    /// Ask a question programmatically.
    pub fn ask(&mut self, prompt: impl Into<String>, cx: &mut Context<Self>) {
        self.send_prompt(prompt.into(), true, cx);
    }

    fn set_status(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = status.into();
        cx.emit(AgentPaneEvent::Status(self.status.clone()));
        cx.notify();
    }

    fn controller(&self, cx: &Context<Self>) -> Entity<RagController> {
        self.state.read(cx).controller.clone()
    }

    fn send_prompt(&mut self, prompt: String, echo_user: bool, cx: &mut Context<Self>) {
        if self.streaming || prompt.trim().is_empty() {
            return;
        }
        let edit_target = self.edit_target.take();
        let enrich_target = self.enrich_target.take();
        let selections = std::mem::take(&mut self.selections);
        let target = edit_target
            .as_ref()
            .and_then(|id| selections.iter().find(|selection| &selection.id == id));
        let enriching = enrich_target.is_some() && target.is_some();
        let exclude = if enriching {
            edit_target.as_deref()
        } else {
            None
        };
        let (mut attachments, mut notes) = self.build_context(&selections, exclude, cx);
        if target.is_some() {
            notes.push("Direct edit: the answer will replace the marked region.".to_owned());
        }
        if enriching {
            notes.push(
                "Enrich mode: the marked region was added to the retrieval query.".to_owned(),
            );
        }
        attachments.append(&mut self.attachments);
        let attachment_count = attachments.len();

        if echo_user {
            self.chat.update(cx, |chat, cx| {
                chat.push_user(prompt.clone(), cx);
            });
        }
        let message = self.chat.update(cx, |chat, cx| chat.begin_assistant(cx));
        if !notes.is_empty() {
            self.chat.update(cx, |chat, cx| {
                chat.push_system_before(message, notes.join("\n"), cx);
            });
        }

        let model_prompt = if let Some(target) = target {
            let range = target.context.range().unwrap_or(0..0);
            if enriching {
                format!(
                    "{prompt}\n\nThe edit target is the marked selection at range {}..{} (digest {}). The marked passage is also the retrieval query, so the corpus context is about it. Enrich the passage with the retrieved evidence and citations, keeping the author's voice. Return exactly one sciscraper:v1:replace block for that selection.",
                    range.start,
                    range.end,
                    target.context.digest()
                )
            } else {
                format!(
                    "{prompt}\n\nThe edit target is the marked selection at range {}..{} (digest {}). Return exactly one sciscraper:v1:replace block for that selection.",
                    range.start,
                    range.end,
                    target.context.digest()
                )
            }
        } else {
            prompt
        };
        let retrieval = if enriching {
            target.map(|target| target.context.text().to_owned())
        } else {
            None
        };
        let controller = self.controller(cx);
        match controller.update(cx, |controller, cx| {
            controller.ask_with_retrieval(model_prompt, retrieval, attachments, cx)
        }) {
            Ok(request) => {
                self.pending = Some(PendingTurn {
                    request,
                    message,
                    tokens: 0,
                    target: target.cloned(),
                });
                self.streaming = true;
                self.chat.update(cx, |chat, cx| chat.scroll_to_end(cx));
                self.set_status(
                    if attachment_count > 0 {
                        "Generating… (with editor context)"
                    } else {
                        "Generating…"
                    },
                    cx,
                );
            }
            Err(error) => {
                self.chat
                    .update(cx, |chat, cx| chat.fail(message, error.clone(), cx));
                self.set_status(error, cx);
            }
        }
        cx.notify();
    }

    fn build_context(
        &mut self,
        selections: &[PendingSelection],
        exclude: Option<&str>,
        cx: &mut Context<Self>,
    ) -> (Vec<PendingAttachment>, Vec<String>) {
        // Collect the editor facts first, then release the state borrow: the
        // contexts themselves are built from owned text so a long document
        // never keeps the entity locked while the prompt is assembled.
        let (document_attached, library_attached, document, bibliography, context_tokens) = {
            let state = self.state.read(cx);
            let document = self.roundtrip.active_document(cx);
            let bibliography = state
                .bibliography
                .clone()
                .and_then(|path| std::fs::read_to_string(&path).ok().map(|text| (path, text)));
            (
                state.document_attached,
                state.library_attached,
                document,
                bibliography,
                state.rag.read(cx).context_tokens,
            )
        };

        let selected = selections
            .iter()
            .filter(|selection| Some(selection.id.as_str()) != exclude)
            .count();
        let items = usize::from(document_attached && document.is_some())
            + usize::from(library_attached && bibliography.is_some())
            + selected;
        let limit = context_char_limit(context_tokens, items);

        let mut attachments = Vec::new();
        let mut notes: Vec<String> = Vec::new();

        if document_attached
            && let Some(document) = document
            && let Some(context) = RoundtripContext::document_with_limit(document.buffer(), limit)
        {
            notes.push(context_note(&context.note()));
            attachments.push(PendingAttachment::text(
                "context-document",
                context.attachment_label(),
                context.container(),
            ));
        }

        if library_attached
            && let Some((path, text)) = bibliography
            && let Some(context) = RoundtripContext::library_with_limit(&path, &text, limit)
        {
            notes.push(context_note(&context.note()));
            attachments.push(PendingAttachment::text(
                "context-library",
                context.attachment_label(),
                context.container(),
            ));
        }

        for selection in selections
            .iter()
            .filter(|selection| Some(selection.id.as_str()) != exclude)
        {
            let context = selection.context.shortened(limit);
            notes.push(format!(
                "Marked region attached: {} ({} chars).",
                context.attachment_label().trim_start_matches("selection "),
                context.chars()
            ));
            attachments.push(PendingAttachment::text(
                selection.id.clone(),
                context.attachment_label(),
                context.container(),
            ));
        }

        (attachments, notes)
    }

    fn on_composer(&mut self, event: ComposerEvent, cx: &mut Context<Self>) {
        match event {
            ComposerEvent::Submit(text) => self.send_prompt(text, true, cx),
            ComposerEvent::Stop => self.stop(cx),
        }
    }

    fn on_chat_event(&mut self, event: ChatViewEvent, cx: &mut Context<Self>) {
        match event {
            ChatViewEvent::Retry(id) => {
                let this = cx.entity();
                cx.defer(move |cx| {
                    this.update(cx, |this, cx| this.retry(id, cx));
                });
            }
            ChatViewEvent::SourceClicked(source) => {
                let location = source
                    .location
                    .clone()
                    .map(|location| format!(" · {location}"))
                    .unwrap_or_default();
                self.set_status(format!("Source: {}{location}", source.title), cx);
            }
            ChatViewEvent::Copied(_) => self.set_status("Message copied", cx),
        }
    }

    fn retry(&mut self, id: MessageId, cx: &mut Context<Self>) {
        if self.streaming {
            return;
        }
        let prompt = {
            let state = self.chat.read(cx).state();
            let state = state.read(cx);
            let messages = state.messages();
            match messages.iter().position(|message| message.id() == id) {
                Some(index) if index + 1 == messages.len() => messages[..index]
                    .iter()
                    .rev()
                    .find(|message| message.role() == ChatRole::User)
                    .map(|message| message.text().to_owned()),
                _ => None,
            }
        };
        let Some(prompt) = prompt else {
            self.set_status("Only the last failed answer can be retried.", cx);
            return;
        };
        self.chat.update(cx, |chat, cx| {
            chat.remove(id, cx);
        });
        self.send_prompt(prompt, false, cx);
    }

    fn on_rag_event(&mut self, event: RagEvent, cx: &mut Context<Self>) {
        match event {
            RagEvent::Started { id } => {
                if self.matches(id) {
                    self.streaming = true;
                    self.composer
                        .update(cx, |composer, cx| composer.set_streaming(true, cx));
                    self.set_status("Generating…", cx);
                }
            }
            RagEvent::Chunk { id, delta } => {
                if let Some(pending) = &mut self.pending
                    && pending.request == id
                {
                    pending.tokens = pending
                        .tokens
                        .saturating_add(delta.split_whitespace().count());
                    self.chat
                        .update(cx, |chat, cx| chat.append(pending.message, &delta, cx));
                }
            }
            RagEvent::Finished { id, reply } => {
                if let Some(pending) = self.pending.take().filter(|pending| pending.request == id) {
                    if pending.tokens == 0 && !reply.answer.is_empty() {
                        self.chat.update(cx, |chat, cx| {
                            chat.append(pending.message, &reply.answer, cx);
                        });
                    }
                    let meta = reply_meta(&reply, pending.tokens);
                    self.chat.update(cx, |chat, cx| {
                        chat.set_meta(pending.message, meta, cx);
                        chat.finish(pending.message, cx);
                    });
                    self.finish_streaming(cx);
                    self.handle_finished(reply, pending, cx);
                }
            }
            RagEvent::Stopped { id, .. } => {
                if let Some(pending) = self.pending.take().filter(|pending| pending.request == id) {
                    self.chat
                        .update(cx, |chat, cx| chat.stop(pending.message, cx));
                    self.finish_streaming(cx);
                    self.set_status("Stopped", cx);
                }
            }
            RagEvent::Failed { id, message } => {
                if let Some(pending) = self.pending.take().filter(|pending| pending.request == id) {
                    self.chat.update(cx, |chat, cx| {
                        chat.fail(pending.message, message.clone(), cx);
                    });
                    self.finish_streaming(cx);
                    self.set_status(message, cx);
                }
            }
            RagEvent::SessionStarted { .. } => {
                self.pending = None;
                self.streaming = false;
                self.attachments.clear();
                self.selections.clear();
                self.edit_target = None;
                self.enrich_target = None;
                self.offer = None;
                self.chat.update(cx, |chat, cx| chat.clear(cx));
                self.composer
                    .update(cx, |composer, cx| composer.set_streaming(false, cx));
            }
            RagEvent::SessionLoaded { turns, .. } => {
                self.pending = None;
                self.streaming = false;
                self.attachments.clear();
                self.selections.clear();
                self.edit_target = None;
                self.enrich_target = None;
                self.offer = None;
                self.chat
                    .update(cx, |chat, cx| chat.replace(loaded_messages(turns), cx));
                self.composer
                    .update(cx, |composer, cx| composer.set_streaming(false, cx));
            }
            RagEvent::AgentReady(info) => {
                self.set_status(format!("{} ready", info.chat_model), cx);
            }
            RagEvent::AgentFailed { message } => self.set_status(message, cx),
            RagEvent::SyncFinished { summary, .. } => {
                self.set_status(
                    format!(
                        "Indexed {} document(s), {} chunk(s) from {}",
                        summary.documents, summary.chunks, summary.corpus
                    ),
                    cx,
                );
            }
            RagEvent::SyncStopped { .. } => self.set_status("Indexing stopped", cx),
            RagEvent::SyncFailed { message, .. } => self.set_status(message, cx),
            RagEvent::Notice(message) => self.set_status(message, cx),
            RagEvent::Citations { resolved, missed } => {
                if let Some(note) = citation_note(&resolved, &missed) {
                    let before = self.pending.as_ref().map(|pending| pending.message);
                    self.chat.update(cx, |chat, cx| {
                        let inserted = before.is_some_and(|before| {
                            chat.push_system_before(before, note.clone(), cx)
                        });
                        if !inserted {
                            chat.push_system(note, cx);
                        }
                    });
                }
            }
            RagEvent::ReferencesStarted { .. }
            | RagEvent::ReferencesProgress { .. }
            | RagEvent::ReferencesLoaded { .. }
            | RagEvent::ReferencesFinished { .. }
            | RagEvent::ReferencesStopped { .. }
            | RagEvent::ReferencesFailed { .. }
            | RagEvent::Sessions(_)
            | RagEvent::Library(_)
            | RagEvent::SyncStarted { .. }
            | RagEvent::SyncProgress { .. } => {}
        }
        cx.notify();
    }

    fn handle_finished(&mut self, reply: RagReply, pending: PendingTurn, cx: &mut Context<Self>) {
        let direct = pending.target.is_some();
        let Some((prose, blocks)) = Replacement::extract(&reply.answer) else {
            if direct {
                self.chat.update(cx, |chat, cx| {
                    chat.push_system(
                        "Direct edit: the answer carried no replacement block; nothing was written.",
                        cx,
                    );
                });
                self.set_status("Direct edit: no replacement block returned", cx);
            } else {
                self.set_status("Answer complete", cx);
            }
            return;
        };
        let empty_prose = prose.trim().is_empty();
        let display = if empty_prose {
            "Replacement ready.".to_owned()
        } else {
            prose
        };
        self.chat.update(cx, |chat, cx| {
            chat.set_message_text(pending.message, display, cx);
        });

        let Some(block) = blocks.first() else {
            self.set_status("Answer complete", cx);
            return;
        };
        let extra = blocks.len().saturating_sub(1);
        if let Some(target) = pending.target {
            let target = ReplacementTarget {
                document_id: target.tab_id,
                range: target.context.range().unwrap_or(0..0),
                digest: target.context.digest().to_owned(),
            };
            self.pending_write = Some(PendingWrite {
                target,
                text: block.body.clone(),
                message: pending.message,
                empty_prose,
            });
            if empty_prose {
                self.chat.update(cx, |chat, cx| {
                    chat.set_message_text(pending.message, "Replacement ready.", cx);
                });
            }
            self.set_status("Applying replacement…", cx);
        } else {
            self.offer = Some(ReplacementOffer {
                text: block.body.clone(),
                reason: "no direct-edit target was armed".to_owned(),
            });
            self.set_status("Replacement ready — Insert above the composer", cx);
        }
        if extra > 0 {
            self.chat.update(cx, |chat, cx| {
                chat.push_system(
                    format!("{extra} further replacement block(s) were ignored."),
                    cx,
                );
            });
        }
    }

    /// Apply a direct-edit answer now that a window is available.
    fn apply_pending_write(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_write.take() else {
            return;
        };
        let outcome = self
            .roundtrip
            .apply(pending.target, &pending.text, window, cx);
        match outcome {
            ReplacementOutcome::Applied { message } => {
                if pending.empty_prose {
                    self.chat.update(cx, |chat, cx| {
                        chat.set_message_text(pending.message, "Replacement applied.", cx);
                    });
                }
                self.chat
                    .update(cx, |chat, cx| chat.push_system(message.clone(), cx));
                self.set_status(message, cx);
            }
            ReplacementOutcome::Offered { text, reason } => {
                if pending.empty_prose {
                    self.chat.update(cx, |chat, cx| {
                        chat.set_message_text(pending.message, "Replacement ready.", cx);
                    });
                }
                self.chat.update(cx, |chat, cx| {
                    chat.push_system(
                        format!(
                            "Replacement not applied: {reason}. Use Insert above the composer."
                        ),
                        cx,
                    );
                });
                self.offer = Some(ReplacementOffer { text, reason });
                self.set_status("Replacement ready — Insert above the composer", cx);
            }
            ReplacementOutcome::Empty => {
                self.chat.update(cx, |chat, cx| {
                    chat.push_system(
                        "The answer carried an empty replacement; nothing was written.",
                        cx,
                    );
                });
                self.set_status("Answer complete", cx);
            }
        }
    }

    fn finish_streaming(&mut self, cx: &mut Context<Self>) {
        self.streaming = false;
        self.composer
            .update(cx, |composer, cx| composer.set_streaming(false, cx));
    }

    fn matches(&self, request: RequestId) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.request == request)
    }

    fn chips(&self, cx: &Context<Self>) -> Vec<Attachment> {
        let state = self.state.read(cx);
        let mut chips = Vec::new();
        let document_label = self
            .roundtrip
            .active_document(cx)
            .map(|document| document.name)
            .unwrap_or_else(|| "No document".into());
        chips.push(
            Attachment::document("context-document", document_label)
                .on(state.document_attached)
                .enabled(state.active_document().is_some())
                .hint("Attach the active document to the next question"),
        );
        if let Some(bibliography) = &state.bibliography {
            let name = bibliography
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| bibliography.display().to_string());
            chips.push(
                Attachment::bibliography("context-library", name)
                    .on(state.library_attached)
                    .hint("Attach the bibliography to the next question"),
            );
        }
        if let Some(target_id) = &self.edit_target
            && let Some(selection) = self
                .selections
                .iter()
                .find(|selection| &selection.id == target_id)
        {
            chips.push(
                Attachment::pending(
                    "edit-mode",
                    format!("Direct edit → {} chars", selection.context.chars()),
                )
                .hint(format!(
                    "The next answer replaces {}",
                    selection.context.attachment_label()
                )),
            );
        }
        for (index, selection) in self.selections.iter().enumerate() {
            chips.push(
                Attachment::pending(
                    selection.id.clone(),
                    format!(
                        "Marked region {} ({} chars)",
                        index + 1,
                        selection.context.chars()
                    ),
                )
                .hint(format!(
                    "The next question uses {}",
                    selection.context.attachment_label()
                )),
            );
        }
        if let Some(offer) = &self.offer {
            chips.push(
                Attachment::offer(
                    "replacement-offer",
                    format!("Replacement ready ({} chars)", offer.text.chars().count()),
                    "Insert",
                )
                .hint(offer.reason.clone()),
            );
        }
        for attachment in &self.attachments {
            chips.push(Attachment::pending(
                attachment.id.clone(),
                attachment.label.clone(),
            ));
        }
        chips
    }

    fn on_attachment_event(
        &mut self,
        event: AttachmentEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            AttachmentEvent::Toggled(id) if id == "context-document" => {
                let next = !self.state.read(cx).document_attached;
                self.state.update(cx, |state, cx| {
                    state.document_attached = next;
                    state.persist(cx);
                    cx.notify();
                });
            }
            AttachmentEvent::Toggled(id) if id == "context-library" => {
                let next = !self.state.read(cx).library_attached;
                self.state.update(cx, |state, cx| {
                    state.library_attached = next;
                    state.persist(cx);
                    cx.notify();
                });
            }
            AttachmentEvent::Dropped(id) if id == "edit-mode" => {
                self.edit_target = None;
                self.set_status("Direct edit cancelled", cx);
            }
            AttachmentEvent::Dropped(id) if id.starts_with("selection-") => {
                let was_target = self.edit_target.as_deref() == Some(id.as_str());
                self.selections.retain(|selection| selection.id != id);
                if was_target {
                    self.edit_target = None;
                }
                self.set_status("Marked region dropped", cx);
            }
            AttachmentEvent::Dropped(id) if id == "replacement-offer" => {
                self.offer = None;
                self.set_status("Replacement dropped", cx);
            }
            AttachmentEvent::Dropped(id) => {
                self.attachments.retain(|attachment| attachment.id != id);
                cx.notify();
            }
            AttachmentEvent::Accepted(id) if id == "replacement-offer" => {
                if let Some(offer) = self.offer.take() {
                    let message = self
                        .state
                        .update(cx, |state, cx| state.insert_offer(&offer.text, window, cx));
                    self.chat
                        .update(cx, |chat, cx| chat.push_system(message.clone(), cx));
                    self.set_status(message, cx);
                }
            }
            AttachmentEvent::Toggled(_) | AttachmentEvent::Accepted(_) => {}
        }
    }
}

impl Render for AgentPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending_write(window, cx);
        let controller = self.controller(cx);
        let (sessions, current, busy, components, status) = {
            let controller = controller.read(cx);
            (
                controller.sessions().to_vec(),
                controller.current_session().map(str::to_owned),
                controller.is_busy() || controller.is_ingesting(),
                controller.agent_components(),
                controller.status_line(),
            )
        };
        let picker = SessionPicker::new("sciscraper-sessions", sessions)
            .busy(busy)
            .current(current.unwrap_or_default())
            .on_event({
                let controller = controller.clone();
                move |event, _, cx| match event {
                    SessionPickerEvent::New => {
                        controller.update(cx, |controller, cx| controller.new_session(cx));
                    }
                    SessionPickerEvent::Load(id) => {
                        controller.update(cx, |controller, cx| controller.load_session(id, cx));
                    }
                    SessionPickerEvent::Refresh => {
                        controller.update(cx, |controller, cx| controller.refresh_sessions(cx));
                    }
                }
            });

        let chips = self.chips(cx);
        let attachment_rows = (!chips.is_empty()).then(|| {
            let this = cx.entity();
            AttachmentRows::new(chips).on_event(move |event, window, cx| {
                this.update(cx, |this, cx| this.on_attachment_event(event, window, cx));
            })
        });

        let agent_bar = AgentBar::new("sciscraper-agent-bar", components).on_event({
            let this = cx.entity();
            move |event, _, cx| {
                if matches!(
                    event,
                    AgentBarEvent::Configure(_) | AgentBarEvent::OpenSettings
                ) {
                    this.update(cx, |_, cx| cx.emit(AgentPaneEvent::OpenSettings));
                }
            }
        });

        let theme = cx.theme();
        v_flex()
            .size_full()
            .min_w(px(340.))
            .bg(theme.background)
            .border_l_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .gap_2()
                    .px_3()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(div().text_lg().child("Scico"))
                            .child(picker),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if controller.read(cx).error().is_some() {
                                theme.danger
                            } else {
                                theme.muted_foreground
                            })
                            .child(if self.status.is_empty() {
                                status
                            } else {
                                self.status.to_string()
                            }),
                    ),
            )
            .child(div().flex_1().min_h(px(0.)).child(self.chat.clone()))
            .when_some(attachment_rows, |this, rows| {
                this.child(div().px_3().py_2().child(rows))
            })
            .child(self.composer.clone())
            .child(
                div()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(agent_bar),
            )
    }
}

impl EventEmitter<AgentPaneEvent> for AgentPane {}

impl Focusable for AgentPane {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn reply_meta(reply: &RagReply, streamed_tokens: usize) -> MessageMeta {
    let tokens = streamed_tokens.max(reply.answer.split_whitespace().count());
    MessageMeta {
        model: Some(reply.model.clone()),
        chunks: reply.chunks,
        sources: reply.sources.clone(),
        tokens: (tokens > 0).then_some(tokens),
        elapsed: reply.elapsed,
    }
}

fn loaded_messages(turns: Vec<SessionTurn>) -> Vec<NewMessage> {
    turns
        .into_iter()
        .map(|turn| match turn.role {
            ChatRole::User => NewMessage::user(turn.text),
            ChatRole::Assistant => NewMessage::assistant(turn.text),
            ChatRole::System => NewMessage::system(turn.text),
        })
        .collect()
}

/// Characters one editor container may spend, leaving room for explicit
/// file attachments and retrieval in the worker's context block.
fn context_char_limit(context_tokens: usize, items: usize) -> usize {
    let context_chars = context_tokens.saturating_sub(1_024).saturating_mul(3);
    let attachment_chars = context_chars * 70 / 100;
    let editor_chars = attachment_chars * 60 / 100;
    (editor_chars / items.max(1)).clamp(600, MAX_CONTEXT_CHARS)
}

fn citation_note(resolved: &[CitationAttachment], missed: &[String]) -> Option<String> {
    if resolved.is_empty() && missed.is_empty() {
        return None;
    }
    let mut lines = Vec::with_capacity(missed.len() + 1);
    if !resolved.is_empty() {
        let keys = resolved
            .iter()
            .map(|attachment| format!("@{}", attachment.key))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("Attached: {keys}"));
    }
    lines.extend(missed.iter().cloned());
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::context_char_limit;

    #[test]
    fn context_limits_shrink_with_the_model_window_and_share() {
        let one = context_char_limit(4_096, 1);
        let two = context_char_limit(4_096, 2);
        let large = context_char_limit(409_600, 1);
        assert!(one > 1_000);
        assert!(two < one);
        assert_eq!(large, 100_000, "the protocol cap still applies");
        assert!(context_char_limit(512, 3) >= 600, "never vanish");
    }
}
