//! Environment bootstrap for GUI launches.
//!
//! Desktop sessions on Linux do not source the user's shell startup files, so
//! variables exported in `~/.bashrc` or `~/.zshrc` never reach applications
//! started from the desktop menu. That includes the provider API keys Ragrig
//! reads from the environment. Windows does not have this gap because `setx`
//! writes the user environment that Explorer passes on.
//!
//! As a fallback, Scico reads `env` from its platform config directory before
//! the first thread starts:
//!
//! - Linux: `~/.config/scico/env`
//! - Windows: `%APPDATA%\scico\env`
//! - macOS: `~/Library/Application Support/scico/env`
//!
//! `SCICO_ENV_FILE` overrides the path. The file holds one `NAME=VALUE` per
//! line; blank lines and lines starting with `#` are ignored, and an optional
//! `export ` prefix is accepted. Variables already present in the process
//! environment always win, so a terminal launch behaves exactly as before.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

/// Environment variable that points at an alternative environment file.
pub const ENV_FILE_OVERRIDE: &str = "SCICO_ENV_FILE";

const ENV_FILE_NAME: &str = "env";

/// Read the user's environment file if one exists.
///
/// Call this once at the top of `main`, before any thread starts. After that,
/// mutating the process environment is no longer safe.
pub fn load() {
    let Some(path) = env_file_path() else {
        return;
    };
    if let Err(error) = load_from(&path)
        && error.kind() != io::ErrorKind::NotFound
    {
        eprintln!(
            "failed to read the Scico environment file '{}': {error}",
            path.display()
        );
    }
}

/// The environment file Scico reads at startup.
pub fn env_file_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os(ENV_FILE_OVERRIDE).map(PathBuf::from)
        && !path.as_os_str().is_empty()
    {
        return Some(path);
    }
    dirs::config_dir().map(|dir| dir.join("scico").join(ENV_FILE_NAME))
}

fn load_from(path: &Path) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    for (name, value) in entries_to_apply(parse(&text), |name| env::var_os(name).is_some()) {
        // SAFETY: `load` runs at the top of `main`, before any thread is
        // spawned. No other thread can observe the environment concurrently.
        unsafe { env::set_var(name, value) };
    }
    Ok(())
}

/// Parse the simple `NAME=VALUE` format described in the module docs.
fn parse(text: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line
            .trim_start_matches('\u{feff}')
            .trim_start()
            .trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = match line.strip_prefix("export") {
            Some(rest) if rest.starts_with(|c: char| c.is_whitespace()) => rest.trim_start(),
            _ => line,
        };
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !valid_name(name) {
            continue;
        }
        let value = unquote(value.trim());
        if value.contains('\0') {
            continue;
        }
        entries.push((name.to_owned(), value.to_owned()));
    }
    entries
}

/// Keep only entries whose variable is not already set in the environment.
fn entries_to_apply(
    entries: Vec<(String, String)>,
    is_set: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    entries
        .into_iter()
        .filter(|(name, _)| !is_set(name))
        .collect()
}

fn valid_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && characters.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|value| value.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{entries_to_apply, parse};

    #[test]
    fn reads_exports_quotes_and_comments() {
        let entries = parse(
            "\
             # provider keys\n\
             export DEEPSEEK_API_KEY=\"sk-deepseek\"\n\
             \n\
             OPENROUTER_API_KEY='sk-or'\n\
             PLAIN=value with spaces\n\
             EMPTY=\n",
        );

        assert_eq!(
            entries,
            vec![
                ("DEEPSEEK_API_KEY".to_owned(), "sk-deepseek".to_owned()),
                ("OPENROUTER_API_KEY".to_owned(), "sk-or".to_owned()),
                ("PLAIN".to_owned(), "value with spaces".to_owned()),
                ("EMPTY".to_owned(), String::new()),
            ]
        );
    }

    #[test]
    fn ignores_lines_that_are_not_assignments() {
        let entries = parse(
            "\
             just text\n\
             =no-name\n\
             1INVALID=value\n\
             NAME-WITH-DASH=value\n\
             exportED=value\n\
             NO_EQUALS\n",
        );

        assert_eq!(entries, vec![("exportED".to_owned(), "value".to_owned())]);
    }

    #[test]
    fn the_process_environment_is_never_overwritten() {
        let entries = parse("EXPORTED=file\nMISSING=file\n");
        let applied = entries_to_apply(entries, |name| name == "EXPORTED");

        assert_eq!(applied, vec![("MISSING".to_owned(), "file".to_owned())]);
    }

    #[test]
    fn a_missing_file_is_reported_as_not_found() {
        let path = std::env::temp_dir().join(format!(
            "scico-env-missing-{}-{}",
            std::process::id(),
            line!()
        ));
        assert_eq!(
            super::load_from(&path).expect_err("missing file").kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
