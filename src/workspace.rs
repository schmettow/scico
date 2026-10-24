//! The Scico workspace layout.
//!
//! A workspace is one directory:
//!
//! ```text
//! my-paper/
//!   Untitled-1.md        the paper
//!   corpus/              source documents (PDFs, Markdown, …)
//!   my-paper.bib         the workspace bibliography
//!   Chat.md             optional project-specific prompt
//!   .ragrig/             vector store, sessions, and profiles
//!   .scico/
//!     workspace.toml     workspace descriptor
//! ```
//!
//! [`prepare`] creates the missing pieces when a workspace is opened, and
//! [`prepare`] with `create` scaffolds a brand-new directory. The descriptor
//! records the discovered bibliography, documents, homepage, and the Ragrig
//! data folder so later opens do not have to rediscover them.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The workspace metadata folder.
pub const WORKSPACE_DIR: &str = ".scico";
/// The workspace descriptor file inside [`WORKSPACE_DIR`].
pub const WORKSPACE_FILE: &str = "workspace.toml";
/// The project-specific Ragrig data folder.
pub const RAGRIG_DIR: &str = ".ragrig";
/// The folder scanned and indexed by the library.
pub const CORPUS_DIR: &str = "corpus";
/// The optional project-specific Chat prompt.
pub const PROMPT_FILE: &str = "Chat.md";
/// The document opened when a workspace has none.
pub const DEFAULT_DOCUMENT: &str = "Untitled-1.md";
/// What to tell the user after a workspace is ready.
pub const INSTRUCTION: &str = "Add PDFs to the library and chat about them.";

/// The first command-line argument when it names an existing directory.
///
/// `args` includes the program name, as `std::env::args_os()` does. A missing
/// argument or a path that is not a directory yields `None`, so a plain
/// launch behaves as before.
pub fn directory_argument(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(args.into_iter().nth(1)?);
    path.is_dir().then_some(path)
}

/// The workspace descriptor stored in `.scico/workspace.toml`.
///
/// Paths are relative to the workspace root. The file is written after every
/// open so it always describes what is actually on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSettings {
    /// Bibliographies found in (or selected for) this workspace.
    pub bibtex: Vec<String>,
    /// Markdown documents that belong to the workspace.
    pub document: Vec<String>,
    /// Where the vector store, sessions, and profiles live.
    pub ragrig: String,
    /// The document opened when the workspace is entered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            bibtex: Vec::new(),
            document: Vec::new(),
            ragrig: format!("./{RAGRIG_DIR}"),
            homepage: None,
        }
    }
}

/// What [`prepare`] found, created, and opened.
#[derive(Clone, Debug)]
pub struct WorkspaceReport {
    /// The workspace root.
    pub root: PathBuf,
    /// Human-readable relative names of everything created.
    pub created: Vec<String>,
    /// The bibliography handed to the agent and the library.
    pub bibliography: PathBuf,
    /// Every workspace document.
    pub documents: Vec<PathBuf>,
    /// The document the editor should show.
    pub homepage: PathBuf,
    /// The project-specific prompt, when `Chat.md` exists.
    pub prompt: Option<PathBuf>,
    /// The Ragrig data folder (vector store, sessions, profiles).
    pub ragrig_dir: PathBuf,
    /// The descriptor that was written.
    pub settings_path: PathBuf,
}

impl WorkspaceReport {
    /// The feedback line shown in the status bar.
    pub fn status(&self, created_workspace: bool) -> String {
        let message = if self.created.is_empty() {
            if created_workspace {
                "Workspace created — everything was already in place".to_owned()
            } else {
                "Workspace opened — nothing was missing".to_owned()
            }
        } else if created_workspace {
            format!("Workspace created: {}", self.created.join(", "))
        } else {
            format!("Created: {}", self.created.join(", "))
        };
        format!("{message} · {INSTRUCTION}")
    }
}

/// Create or open `root` and return the complete workspace layout.
///
/// With `create`, `root` must not exist or must be an empty directory. Without
/// it, `root` must already be a directory. In both cases every missing part of
/// the scaffold is added and reported. A user-visible error is returned when a
/// file cannot be created or the descriptor cannot be parsed.
pub fn prepare(root: &Path, create: bool) -> Result<WorkspaceReport, String> {
    let root = prepare_root(root, create)?;
    let mut created = Vec::new();

    let settings_dir = root.join(WORKSPACE_DIR);
    let settings_path = settings_dir.join(WORKSPACE_FILE);
    let settings_existed = settings_path.is_file();
    ensure_dir(&settings_dir, &root, &mut created)?;
    let mut settings = if settings_existed {
        let text = fs::read_to_string(&settings_path)
            .map_err(|error| format!("cannot read '{}': {error}", settings_path.display()))?;
        toml::from_str::<WorkspaceSettings>(&text).map_err(|error| {
            format!(
                "cannot parse '{}': {error} — fix or remove the file",
                settings_path.display()
            )
        })?
    } else {
        WorkspaceSettings::default()
    };

    let ragrig_dir = resolve(&root, &settings.ragrig);
    ensure_dir(&ragrig_dir, &root, &mut created)?;
    ensure_dir(&ragrig_dir.join("profiles"), &root, &mut created)?;
    ensure_dir(&ragrig_dir.join("sessions"), &root, &mut created)?;
    migrate_legacy_store(&root, &ragrig_dir, &mut created);

    ensure_dir(&root.join(CORPUS_DIR), &root, &mut created)?;

    let bibtex = ensure_bibliography(&root, &settings, &mut created)?;
    let (documents, homepage) = ensure_documents(&root, &settings, &mut created)?;

    let prompt_path = root.join(PROMPT_FILE);
    if create && !prompt_path.is_file() {
        fs::write(&prompt_path, crate::settings::CHAT_PROMPT)
            .map_err(|error| format!("cannot write '{}': {error}", prompt_path.display()))?;
        created.push(PROMPT_FILE.to_owned());
    }
    let prompt = prompt_path.is_file().then_some(prompt_path);

    let bibtex_names = bibtex.iter().map(|path| relative(&root, path)).collect();
    let document_names = documents.iter().map(|path| relative(&root, path)).collect();
    let homepage_name = relative(&root, &homepage);
    let ragrig_name = ragrig_dir
        .strip_prefix(&root)
        .map(|path| format!("./{}", path.to_string_lossy().replace('\\', "/")))
        .unwrap_or_else(|_| ragrig_dir.display().to_string());
    let changed = settings.bibtex != bibtex_names
        || settings.document != document_names
        || settings.homepage.as_deref() != Some(homepage_name.as_str())
        || settings.ragrig != ragrig_name;
    settings.bibtex = bibtex_names;
    settings.document = document_names;
    settings.homepage = Some(homepage_name);
    settings.ragrig = ragrig_name;
    if !settings_existed || changed {
        let text = toml::to_string_pretty(&settings)
            .map_err(|error| format!("cannot serialize the workspace settings: {error}"))?;
        fs::write(&settings_path, text)
            .map_err(|error| format!("cannot write '{}': {error}", settings_path.display()))?;
        if !settings_existed {
            created.push(format!("{WORKSPACE_DIR}/{WORKSPACE_FILE}"));
        }
    }

    Ok(WorkspaceReport {
        root,
        created,
        bibliography: bibtex
            .into_iter()
            .next()
            .expect("ensure_bibliography always returns one"),
        documents,
        homepage,
        prompt,
        ragrig_dir,
        settings_path,
    })
}

fn prepare_root(root: &Path, create: bool) -> Result<PathBuf, String> {
    if root.is_file() {
        return Err(format!(
            "'{}' is a file; choose a folder for the workspace",
            root.display()
        ));
    }
    if create {
        if root.exists() {
            let mut entries = fs::read_dir(root)
                .map_err(|error| format!("cannot read '{}': {error}", root.display()))?;
            if entries.next().is_some() {
                return Err(format!(
                    "'{}' already exists and is not empty — choose a new folder",
                    root.display()
                ));
            }
        } else {
            fs::create_dir_all(root)
                .map_err(|error| format!("cannot create '{}': {error}", root.display()))?;
        }
    } else if !root.is_dir() {
        return Err(format!("'{}' is not a directory", root.display()));
    }
    Ok(fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()))
}

fn ensure_dir(path: &Path, root: &Path, created: &mut Vec<String>) -> Result<(), String> {
    if path.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(path)
        .map_err(|error| format!("cannot create '{}': {error}", path.display()))?;
    let name = relative(root, path);
    if !created.contains(&name) {
        created.push(name);
    }
    Ok(())
}

/// Ragrig used to put its store next to `.ragrig`; move it inside.
fn migrate_legacy_store(root: &Path, ragrig_dir: &Path, created: &mut Vec<String>) {
    let legacy = root.join(".ragrig_store");
    let target = ragrig_dir.join(".ragrig_store");
    if legacy.is_dir() && !target.exists() && fs::rename(&legacy, &target).is_ok() {
        created.push(format!("{} (moved)", relative(root, &target)));
    }
}

fn ensure_bibliography(
    root: &Path,
    settings: &WorkspaceSettings,
    created: &mut Vec<String>,
) -> Result<Vec<PathBuf>, String> {
    let mut found: Vec<PathBuf> = settings
        .bibtex
        .iter()
        .filter_map(|entry| existing(root, entry))
        .collect();
    if found.is_empty() {
        found = scan_files(root, &["bib", "bibtex"]);
    }
    if found.is_empty() {
        let name = bibliography_name(root);
        let path = root.join(&name);
        fs::write(&path, "")
            .map_err(|error| format!("cannot create '{}': {error}", path.display()))?;
        created.push(name);
        found.push(path);
    }
    found.sort();
    found.dedup();
    Ok(found)
}

fn ensure_documents(
    root: &Path,
    settings: &WorkspaceSettings,
    created: &mut Vec<String>,
) -> Result<(Vec<PathBuf>, PathBuf), String> {
    let mut found: Vec<PathBuf> = settings
        .document
        .iter()
        .filter_map(|entry| existing(root, entry))
        .filter(|path| is_document(path))
        .collect();
    if found.is_empty() {
        found = scan_files(root, &["md", "markdown", "qmd"])
            .into_iter()
            .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some(PROMPT_FILE))
            .collect();
    }
    if found.is_empty() {
        let path = root.join(DEFAULT_DOCUMENT);
        fs::write(&path, "")
            .map_err(|error| format!("cannot create '{}': {error}", path.display()))?;
        created.push(DEFAULT_DOCUMENT.to_owned());
        found.push(path);
    }
    found.sort();
    found.dedup();
    let homepage = settings
        .homepage
        .as_deref()
        .and_then(|entry| existing(root, entry))
        .filter(|path| found.contains(path))
        .unwrap_or_else(|| found[0].clone());
    Ok((found, homepage))
}

fn scan_files(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extensions
                        .iter()
                        .any(|wanted| extension.eq_ignore_ascii_case(wanted))
                })
        })
        .collect();
    files.sort();
    files
}

fn is_document(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["md", "markdown", "qmd"]
                .iter()
                .any(|wanted| extension.eq_ignore_ascii_case(wanted))
        })
        && path.file_name().and_then(|name| name.to_str()) != Some(PROMPT_FILE)
}

fn existing(root: &Path, entry: &str) -> Option<PathBuf> {
    let path = resolve(root, entry);
    path.is_file().then_some(path)
}

fn resolve(root: &Path, entry: &str) -> PathBuf {
    let path = Path::new(entry.trim());
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn bibliography_name(root: &Path) -> String {
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("references");
    let sanitized: String = name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect();
    format!("{sanitized}.bib")
}

/// Merge `overlay` into `<root>/.ragrig/config.json`.
///
/// The status-bar switches use this to make their choice part of the
/// workspace layer, so it survives a reset of the global `settings.json`.
/// Existing keys are preserved.
pub fn write_ragrig_layer(root: &Path, overlay: serde_json::Value) -> Result<(), String> {
    let dir = root.join(RAGRIG_DIR);
    fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create '{}': {error}", dir.display()))?;
    let path = dir.join("config.json");
    let mut base = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    merge_json(&mut base, overlay);
    let text = serde_json::to_string_pretty(&base)
        .map_err(|error| format!("cannot serialize '{}': {error}", path.display()))?;
    fs::write(&path, text).map_err(|error| format!("cannot write '{}': {error}", path.display()))
}

fn merge_json(base: &mut serde_json::Value, overlay: serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base), serde_json::Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn scratch(name: &str) -> PathBuf {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "scico-workspace-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn the_first_argument_is_a_workspace_only_when_it_is_a_directory() {
        let root = scratch("arg");
        fs::create_dir_all(&root).expect("root");
        let program = OsString::from("scico");

        assert_eq!(
            directory_argument([program.clone(), root.clone().into_os_string()]),
            Some(root.clone())
        );
        assert_eq!(directory_argument([program.clone()]), None);

        let file = root.join("paper.md");
        fs::write(&file, "# Paper").expect("file");
        assert_eq!(
            directory_argument([program, file.into_os_string()]),
            None,
            "a file argument is not a workspace"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_new_workspace_gets_the_full_scaffold() {
        let root = scratch("new");
        let report = prepare(&root, true).expect("workspace");

        assert!(report.root.join("corpus").is_dir());
        assert!(report.root.join(".ragrig/profiles").is_dir());
        assert!(report.root.join(".ragrig/sessions").is_dir());
        assert!(report.root.join(".scico/workspace.toml").is_file());
        assert!(report.root.join("Untitled-1.md").is_file());
        assert!(report.root.join("Chat.md").is_file());
        assert!(report.bibliography.is_file());
        assert_eq!(report.bibliography.extension().unwrap(), "bib");
        assert_eq!(report.homepage.file_name().unwrap(), DEFAULT_DOCUMENT);
        assert!(report.created.iter().any(|item| item == "corpus"));
        assert!(
            report
                .created
                .iter()
                .any(|item| item == ".scico/workspace.toml")
        );
        assert!(report.created.iter().any(|item| item == DEFAULT_DOCUMENT));

        let settings = fs::read_to_string(report.settings_path).expect("descriptor");
        assert!(settings.contains("document = [\"Untitled-1.md\"]"));
        assert!(settings.contains("homepage = \"Untitled-1.md\""));
        assert!(settings.contains("bibtex = ["));
        assert!(settings.contains("ragrig = \"./.ragrig\""));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn opening_a_workspace_adds_only_the_missing_parts() {
        let root = scratch("open");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("paper.md"), "# Paper").expect("document");
        fs::write(root.join("existing.bib"), "@article{key}").expect("bibliography");

        let report = prepare(&root, false).expect("workspace");
        assert_eq!(report.bibliography.file_name().unwrap(), "existing.bib");
        assert_eq!(report.homepage.file_name().unwrap(), "paper.md");
        assert!(root.join(".ragrig").is_dir());
        assert!(root.join("corpus").is_dir());
        assert!(!root.join("Chat.md").exists(), "the prompt stays optional");
        assert!(
            report.created.iter().any(|item| item == ".ragrig"),
            "only the missing parts are reported"
        );

        let again = prepare(&root, false).expect("workspace");
        assert!(
            !again.created.contains(&"corpus".to_owned()),
            "a second open reports nothing"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_workspace_without_a_bibliography_creates_one_named_after_the_folder() {
        let root = scratch("bib").join("my-paper");
        fs::create_dir_all(&root).expect("root");
        let report = prepare(&root, false).expect("workspace");
        assert_eq!(report.bibliography.file_name().unwrap(), "my-paper.bib");
        assert!(report.bibliography.is_file());
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn the_descriptor_survives_a_round_trip() {
        let root = scratch("roundtrip");
        let first = prepare(&root, true).expect("workspace");
        let text = fs::read_to_string(&first.settings_path).expect("descriptor");
        let settings: WorkspaceSettings = toml::from_str(&text).expect("parse");

        assert_eq!(settings.document, vec![DEFAULT_DOCUMENT.to_owned()]);
        assert_eq!(settings.homepage.as_deref(), Some(DEFAULT_DOCUMENT));
        assert_eq!(settings.ragrig, "./.ragrig");
        assert_eq!(settings.bibtex.len(), 1);

        let second = prepare(&root, false).expect("workspace");
        assert_eq!(second.homepage, first.homepage);
        assert!(second.created.is_empty(), "nothing is created twice");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_existing_non_empty_folder_is_not_overwritten() {
        let root = scratch("occupied");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("keep.txt"), "keep").expect("file");
        let error = prepare(&root, true).expect_err("must refuse");
        assert!(error.contains("not empty"));
        assert!(root.join("keep.txt").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_ragrig_override_merges_into_the_workspace_layer() {
        let root = scratch("override");
        fs::create_dir_all(root.join(RAGRIG_DIR)).expect("ragrig dir");
        fs::write(
            root.join(RAGRIG_DIR).join("config.json"),
            r#"{"chat":{"model":"keep-me"},"memory":{"model":"small"}}"#,
        )
        .expect("existing layer");

        write_ragrig_layer(
            &root,
            serde_json::json!({"memory": {"strategy": "off"}, "hybrid": false}),
        )
        .expect("override");

        let text = fs::read_to_string(root.join(RAGRIG_DIR).join("config.json")).expect("layer");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["chat"]["model"], "keep-me", "other keys survive");
        assert_eq!(value["memory"]["model"], "small", "siblings survive");
        assert_eq!(value["memory"]["strategy"], "off");
        assert_eq!(value["hybrid"], false);
        let _ = fs::remove_dir_all(root);
    }
}
