//! Crash-safe editor sessions.
//!
//! The session file is a journal of what the window was showing: the open
//! documents, their *unsaved* text, the caret, the active tab, and the pane
//! layout. It is separate from [`crate::settings::PersistedSettings`] because
//! it changes on every keystroke.
//!
//! Writes are atomic: the snapshot is written to a sibling temporary file,
//! flushed, and renamed over the session file. A crash can therefore leave the
//! previous complete snapshot or the new one, never a truncated file.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use serde::{Deserialize, Serialize};

/// One open buffer in a session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionDocument {
    /// File backing the document, `None` for an untitled buffer.
    pub path: Option<PathBuf>,
    /// Tab title, used to keep untitled buffers recognisable.
    pub title: String,
    /// The exact buffer source, saved or not.
    pub text: String,
    /// The caret as a byte offset into [`Self::text`].
    pub cursor: usize,
    /// Whether the buffer differed from its file when the snapshot was taken.
    pub dirty: bool,
}

/// Everything needed to rebuild the last window state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    /// Open documents in tab order.
    pub documents: Vec<SessionDocument>,
    /// Index into [`Self::documents`].
    pub active: usize,
    /// Show the workspace pane.
    pub show_library: bool,
    /// Show the Chat pane.
    pub show_agent: bool,
    /// Show the rendered preview beside the source editor.
    pub show_preview: bool,
    /// Keep the source editor and the preview scrolled together.
    pub sync_scroll: bool,
    /// Render raw Markdown rather than the live preview.
    pub raw_mode: bool,
    /// Width of the workspace pane.
    pub library_width: f32,
    /// Width of the Chat pane.
    pub agent_width: f32,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            documents: Vec::new(),
            active: 0,
            show_library: true,
            show_agent: true,
            show_preview: true,
            sync_scroll: true,
            raw_mode: false,
            library_width: 340.0,
            agent_width: 430.0,
        }
    }
}

impl SessionState {
    /// The conventional session path for a settings file.
    ///
    /// Keeping the two next to each other means `SCICO_SETTINGS` also
    /// isolates test and development data.
    pub fn path_for_settings(settings_path: &Path) -> PathBuf {
        settings_path.with_file_name("session.json")
    }

    /// Read a session.
    ///
    /// A missing file is not an error. A corrupt file is renamed to
    /// `session.corrupt.json` before being reported, so the next snapshot
    /// cannot silently destroy the only copy.
    pub fn load(path: &Path) -> (Option<Self>, Option<String>) {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (None, None),
            Err(error) => {
                return (
                    None,
                    Some(format!("cannot read session '{}': {error}", path.display())),
                );
            }
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(session) => (Some(session), None),
            Err(error) => {
                let backup = path.with_file_name("session.corrupt.json");
                let backup_note = match fs::rename(path, &backup) {
                    Ok(()) => format!(" (kept as '{}')", backup.display()),
                    Err(_) => String::new(),
                };
                (
                    None,
                    Some(format!(
                        "cannot parse session '{}': {error}{backup_note}",
                        path.display()
                    )),
                )
            }
        }
    }

    /// Write the session atomically, creating its directory when needed.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create '{}': {error}", parent.display()))?;
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("cannot serialize session: {error}"))?;
        let temporary = path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&temporary)
                .map_err(|error| format!("cannot create '{}': {error}", temporary.display()))?;
            file.write_all(&bytes)
                .map_err(|error| format!("cannot write '{}': {error}", temporary.display()))?;
            // No fsync on the hot path: the process-crash safety comes from the
            // atomic rename, while an fsync per keystroke makes key repeat
            // visibly stutter. The OS flushes the completed file normally.
        }
        fs::rename(&temporary, path)
            .map_err(|error| format!("cannot replace '{}': {error}", path.display()))?;
        Ok(())
    }
}

/// Writes session snapshots in order, usually off the UI thread.
///
/// A snapshot contains every open buffer, so serializing and writing it on the
/// UI thread stalls typing for tens of milliseconds on a large document. The
/// debounced edit path uses [`Self::write`], which only hands the snapshot to a
/// writer thread; explicit actions use [`Self::write_blocking`] so closing and
/// saving stay durable.
///
/// Requests are processed in FIFO order. An explicit write therefore always
/// lands after every edit that was queued before it, and cannot be overwritten
/// by an older background snapshot.
pub struct SessionWriter {
    path: PathBuf,
    sync: bool,
    sender: Option<mpsc::Sender<WriteRequest>>,
    thread: Option<thread::JoinHandle<()>>,
}

struct WriteRequest {
    snapshot: SessionState,
    done: Option<mpsc::Sender<Result<(), String>>>,
}

impl SessionWriter {
    /// Create a writer for `path`.
    ///
    /// `sync` writes on the calling thread. Tests set it so their file
    /// assertions are deterministic; production leaves it `false` and gets a
    /// named writer thread.
    pub fn new(path: PathBuf, sync: bool) -> Self {
        if sync {
            return Self {
                path,
                sync: true,
                sender: None,
                thread: None,
            };
        }

        let (sender, receiver) = mpsc::channel::<WriteRequest>();
        let writer_path = path.clone();
        let spawned = thread::Builder::new()
            .name("scico-session".to_owned())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let result = request.snapshot.save(&writer_path);
                    match (result, request.done) {
                        (result, Some(done)) => {
                            let _ = done.send(result);
                        }
                        (Err(error), None) => {
                            eprintln!("session not saved: {error}");
                        }
                        (Ok(()), None) => {}
                    }
                }
            });

        match spawned {
            Ok(thread) => Self {
                path,
                sync: false,
                sender: Some(sender),
                thread: Some(thread),
            },
            // Without a writer thread the session must still be saved; fall
            // back to the calling thread instead of dropping snapshots.
            Err(_) => Self {
                path,
                sync: true,
                sender: None,
                thread: None,
            },
        }
    }

    /// Queue a snapshot for the writer thread, or write it now in sync mode.
    ///
    /// Errors are reported on stderr in async mode. The caller has already
    /// recorded the snapshot as written, so this is not treated as a fatal
    /// condition on the typing path.
    pub fn write(&self, snapshot: SessionState) {
        if self.sync {
            if let Err(error) = snapshot.save(&self.path) {
                eprintln!("session not saved: {error}");
            }
            return;
        }
        let Some(sender) = &self.sender else {
            if let Err(error) = snapshot.save(&self.path) {
                eprintln!("session not saved: {error}");
            }
            return;
        };
        // A disconnected writer means the thread panicked. `SendError`
        // returns the request, so fall back to a synchronous write rather
        // than silently losing the snapshot.
        if let Err(mpsc::SendError(request)) = sender.send(WriteRequest {
            snapshot,
            done: None,
        }) && let Err(error) = request.snapshot.save(&self.path)
        {
            eprintln!("session not saved: {error}");
        }
    }

    /// Write a snapshot and wait until it is on disk.
    ///
    /// Used by explicit actions (open, save, close, layout) and ordered after
    /// any queued edits.
    pub fn write_blocking(&self, snapshot: SessionState) -> Result<(), String> {
        if self.sync {
            return snapshot.save(&self.path);
        }
        let (done, wait) = mpsc::channel();
        self.sender
            .as_ref()
            .ok_or_else(|| "the session writer is not running".to_owned())?
            .send(WriteRequest {
                snapshot,
                done: Some(done),
            })
            .map_err(|_| "the session writer stopped".to_owned())?;
        wait.recv()
            .map_err(|_| "the session writer stopped".to_owned())?
    }
}

impl Drop for SessionWriter {
    fn drop(&mut self) {
        // Close the queue; the thread drains the snapshots already accepted
        // and then exits. Joining keeps a reopened window from reading a
        // half-finished rename.
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("scico-session-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn sample() -> SessionState {
        SessionState {
            documents: vec![
                SessionDocument {
                    path: Some(PathBuf::from("/tmp/paper.md")),
                    title: "paper.md".to_owned(),
                    text: "# Draft\n\nUnsaved paragraph".to_owned(),
                    cursor: 9,
                    dirty: true,
                },
                SessionDocument {
                    path: None,
                    title: "Untitled-2".to_owned(),
                    text: "notes".to_owned(),
                    cursor: 5,
                    dirty: true,
                },
            ],
            active: 1,
            show_library: false,
            show_agent: true,
            show_preview: false,
            sync_scroll: false,
            raw_mode: true,
            library_width: 280.0,
            agent_width: 512.0,
        }
    }

    #[test]
    fn a_session_round_trips_with_unsaved_text() {
        let dir = scratch();
        let path = dir.join("session.json");
        let session = sample();
        session.save(&path).expect("save");
        let (loaded, error) = SessionState::load(&path);
        assert!(error.is_none(), "{error:?}");
        assert_eq!(loaded, Some(session));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn saving_twice_replaces_the_file_and_leaves_no_temporary() {
        let dir = scratch();
        let path = dir.join("session.json");
        sample().save(&path).expect("first save");
        let mut newer = sample();
        newer.active = 0;
        newer.documents[0].text = "second".to_owned();
        newer.save(&path).expect("second save");
        let (loaded, _) = SessionState::load(&path);
        assert_eq!(loaded.expect("session").documents[0].text, "second");
        assert!(!path.with_extension("json.tmp").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_session_is_not_an_error() {
        let dir = scratch();
        let (session, error) = SessionState::load(&dir.join("session.json"));
        assert!(session.is_none());
        assert!(error.is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_corrupt_session_is_reported_and_kept() {
        let dir = scratch();
        let path = dir.join("session.json");
        fs::write(&path, "{ not json").expect("corrupt");
        let (session, error) = SessionState::load(&path);
        assert!(session.is_none());
        assert!(error.expect("error").contains("cannot parse session"));
        assert!(dir.join("session.corrupt.json").is_file());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_session_sits_beside_the_settings_file() {
        let path = SessionState::path_for_settings(Path::new("/tmp/here/settings.json"));
        assert_eq!(path, PathBuf::from("/tmp/here/session.json"));
    }

    #[test]
    fn an_asynchronous_writer_writes_snapshots_in_order() {
        let dir = scratch();
        let path = dir.join("session.json");
        let writer = SessionWriter::new(path.clone(), false);

        let mut first = sample();
        first.active = 0;
        let mut second = sample();
        second.active = 1;
        second.documents[0].text = "second".to_owned();

        writer.write(first);
        writer.write(second.clone());
        // Waiting for the third write also proves the first two were processed;
        // the queue is FIFO.
        writer.write_blocking(sample()).expect("flush");
        drop(writer);

        let (loaded, error) = SessionState::load(&path);
        assert!(error.is_none(), "{error:?}");
        assert_eq!(loaded, Some(sample()), "the last snapshot wins");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_explicit_write_is_visible_before_it_returns() {
        let dir = scratch();
        let path = dir.join("session.json");
        let writer = SessionWriter::new(path.clone(), false);
        writer.write(sample());
        writer.write_blocking(sample()).expect("explicit write");

        let (loaded, error) = SessionState::load(&path);
        assert!(error.is_none(), "{error:?}");
        assert_eq!(loaded, Some(sample()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_synchronous_writer_keeps_tests_deterministic() {
        let dir = scratch();
        let path = dir.join("session.json");
        let writer = SessionWriter::new(path.clone(), true);
        writer.write(sample());

        let (loaded, _) = SessionState::load(&path);
        assert_eq!(loaded, Some(sample()));
        let _ = fs::remove_dir_all(dir);
    }
}
