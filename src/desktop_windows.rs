//! Desktop integration for Windows launchers.
//!
//! Windows has no `.desktop` files. The Start menu lists shortcuts from
//! `%APPDATA%\Microsoft\Windows\Start Menu\Programs`, and the Start menu,
//! search, and a pinned taskbar entry take the application name and icon from
//! that shortcut. Release builds install `Scico.lnk` there on first launch,
//! together with a standalone `.ico` below `%APPDATA%\scico`, and refresh both
//! when the executable moves. Debug builds skip the installer so a development
//! binary cannot repoint the Start menu entry at `target/debug`.
//!
//! The shortcut is created by `powershell.exe` through `WScript.Shell`, so
//! Scico needs no COM bindings or installer framework. Everything is per-user,
//! needs no administrator rights, and uninstalling is a matter of deleting the
//! shortcut, `%APPDATA%\scico\scico.ico`, and the marker file.

use std::{
    env, fs, io,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    thread,
};

use crate::APP_ID;

const ICON_ICO: &[u8] = include_bytes!("../assets/logo/Scico.ico");
const SHORTCUT_NAME: &str = "Scico.lnk";
const MARKER_NAME: &str = "desktop-installed";
const DESCRIPTION: &str =
    "Scientific Markdown writing with integrated RAG co-writing and reference management";

/// Hide the console window a helper process would otherwise flash when Scico
/// is started from a terminal.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Install or refresh the Start menu shortcut and its icon.
///
/// The shortcut goes through PowerShell, so the work runs on a detached thread
/// and never delays the window. Failures are reported but never stop the
/// editor from starting: a missing launcher entry only costs the menu icon.
pub fn install() {
    let Some(data_dir) = dirs::data_dir() else {
        return;
    };
    let Ok(executable) = env::current_exe() else {
        return;
    };
    thread::spawn(move || {
        if let Err(error) = install_to(&data_dir, &executable) {
            eprintln!("failed to install the Scico Start menu entry: {error}");
        }
    });
}

/// Write the icon, shortcut, and marker below `data_dir`.
///
/// The marker records the executable path, so a moved or rebuilt binary gets a
/// fresh shortcut while ordinary launches skip the PowerShell round trip.
fn install_to(data_dir: &Path, executable: &Path) -> io::Result<()> {
    let icon = icon_path(data_dir);
    write_if_changed(&icon, ICON_ICO)?;

    if shortcut_is_current(data_dir, executable) {
        return Ok(());
    }

    create_shortcut(&shortcut_path(data_dir), executable, &icon)?;
    write_if_changed(
        &marker_path(data_dir),
        executable_signature(executable).as_bytes(),
    )
}

fn shortcut_is_current(data_dir: &Path, executable: &Path) -> bool {
    shortcut_path(data_dir).is_file()
        && fs::read_to_string(marker_path(data_dir))
            .is_ok_and(|existing| existing == executable_signature(executable))
}

fn icon_path(data_dir: &Path) -> PathBuf {
    data_dir.join(APP_ID).join("scico.ico")
}

fn marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(APP_ID).join(MARKER_NAME)
}

/// The per-user Start menu folder, which is the Windows counterpart of the
/// XDG applications directory.
fn shortcut_path(data_dir: &Path) -> PathBuf {
    data_dir
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join(SHORTCUT_NAME)
}

fn executable_signature(executable: &Path) -> String {
    format!("{}\n", executable.display())
}

/// Create the shell link with `WScript.Shell`.
///
/// The script is passed as UTF-16LE base64 so paths need no command-line
/// quoting; single quotes inside the script are escaped the PowerShell way.
fn create_shortcut(shortcut: &Path, executable: &Path, icon: &Path) -> io::Result<()> {
    if let Some(parent) = shortcut.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = run_powershell(&shortcut_script(shortcut, executable, icon))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "powershell.exe exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn run_powershell(script: &str) -> io::Result<std::process::Output> {
    let encoded = encode_command(script);
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            encoded.as_str(),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

fn shortcut_script(shortcut: &Path, executable: &Path, icon: &Path) -> String {
    let working_directory = executable.parent().unwrap_or(Path::new("."));
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $shell = New-Object -ComObject WScript.Shell\n\
         $link = $shell.CreateShortcut({})\n\
         $link.TargetPath = {}\n\
         $link.Arguments = ''\n\
         $link.WorkingDirectory = {}\n\
         $link.IconLocation = {}\n\
         $link.Description = {}\n\
         $link.Save()\n",
        quote(&shortcut.to_string_lossy()),
        quote(&executable.to_string_lossy()),
        quote(&working_directory.to_string_lossy()),
        quote(&format!("{},0", icon.display())),
        quote(DESCRIPTION),
    )
}

/// Quote a literal for a PowerShell single-quoted string; an apostrophe is
/// doubled and nothing else is special.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Encode a script for `powershell.exe -EncodedCommand`: UTF-16LE, then
/// standard base64.
fn encode_command(script: &str) -> String {
    let mut bytes = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0] as u32;
        let second = chunk.get(1).copied().unwrap_or(0) as u32;
        let third = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (first << 16) | (second << 8) | third;

        encoded.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        encoded.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    encoded
}

fn write_if_changed(path: &Path, contents: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        ICON_ICO, encode_command, executable_signature, icon_path, install_to, marker_path,
        shortcut_is_current, shortcut_path, shortcut_script,
    };

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn scratch_dir() -> PathBuf {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "scico-desktop-windows-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn read_shortcut(shortcut: &std::path::Path, property: &str) -> String {
        let script = format!(
            "$shell = New-Object -ComObject WScript.Shell\n\
             $link = $shell.CreateShortcut({})\n\
             Write-Output $link.{property}\n",
            super::quote(&shortcut.to_string_lossy()),
        );
        let output = super::run_powershell(&script).expect("run powershell");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn sample_script() -> String {
        shortcut_script(
            std::path::Path::new(
                r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Scico.lnk",
            ),
            std::path::Path::new(r"C:\Program Files\Scico\scico.exe"),
            std::path::Path::new(r"C:\Users\me\AppData\Roaming\scico\scico.ico"),
        )
    }

    #[test]
    fn the_shortcut_names_the_executable_and_the_icon() {
        let script = sample_script();

        assert!(script.contains(r"$link.TargetPath = 'C:\Program Files\Scico\scico.exe'"));
        assert!(script.contains(r"$link.WorkingDirectory = 'C:\Program Files\Scico'"));
        assert!(
            script
                .contains(r"$link.IconLocation = 'C:\Users\me\AppData\Roaming\scico\scico.ico,0'")
        );
        assert!(script.contains("$link.Description = 'Scientific Markdown"));
        assert!(script.contains("$link.Save()"));
    }

    #[test]
    fn apostrophes_in_paths_are_doubled_for_powershell() {
        let script = shortcut_script(
            std::path::Path::new(r"C:\Users\O'Brien\Start Menu\Scico.lnk"),
            std::path::Path::new(r"C:\Users\O'Brien\scico.exe"),
            std::path::Path::new(r"C:\Users\O'Brien\scico.ico"),
        );

        assert!(script.contains(r"'C:\Users\O''Brien\scico.exe'"));
        assert!(script.contains(r"'C:\Users\O''Brien\scico.ico,0'"));
    }

    #[test]
    fn the_icon_packs_the_windows_sizes() {
        assert_eq!(&ICON_ICO[..4], b"\0\0\x01\0");
        let count = u16::from_le_bytes([ICON_ICO[4], ICON_ICO[5]]) as usize;
        assert_eq!(count, 7);

        let sizes: Vec<u16> = (0..count)
            .map(|index| {
                let width = ICON_ICO[6 + index * 16];
                if width == 0 { 256 } else { u16::from(width) }
            })
            .collect();
        assert_eq!(sizes, [16, 24, 32, 48, 64, 128, 256]);
    }

    #[test]
    fn encoded_commands_are_utf16le_base64() {
        assert_eq!(encode_command("Hi"), "SABpAA==");
        assert_eq!(encode_command(""), "");
        assert_eq!(encode_command("A"), "QQA=");
    }

    #[test]
    fn installation_writes_the_icon_shortcut_and_marker_idempotently() {
        let data_dir = scratch_dir();
        let executable = std::env::current_exe().expect("test executable");

        install_to(&data_dir, &executable).expect("first install");
        install_to(&data_dir, &executable).expect("second install");

        let icon = icon_path(&data_dir);
        assert_eq!(
            std::fs::read(&icon).expect("icon"),
            ICON_ICO,
            "the installed icon is the embedded asset"
        );

        let shortcut = shortcut_path(&data_dir);
        let link = std::fs::read(&shortcut).expect("shortcut");
        assert_eq!(&link[..4], b"L\0\0\0", "a shell link starts with its size");
        assert!(
            shortcut.starts_with(&data_dir),
            "the shortcut stays below the data directory"
        );
        assert_eq!(
            read_shortcut(&shortcut, "TargetPath"),
            executable.display().to_string()
        );
        assert_eq!(
            read_shortcut(&shortcut, "IconLocation"),
            format!("{},0", icon.display())
        );
        assert_eq!(
            read_shortcut(&shortcut, "WorkingDirectory"),
            executable.parent().expect("parent").display().to_string()
        );
        assert!(
            read_shortcut(&shortcut, "Description").starts_with("Scientific Markdown"),
            "the tooltip describes the app"
        );

        let marker = marker_path(&data_dir);
        assert_eq!(
            std::fs::read_to_string(marker).expect("marker"),
            executable_signature(&executable)
        );

        std::fs::remove_dir_all(&data_dir).expect("clean up");
    }

    #[test]
    fn a_moved_executable_makes_the_shortcut_stale() {
        let data_dir = scratch_dir();
        let shortcut = shortcut_path(&data_dir);
        std::fs::create_dir_all(shortcut.parent().expect("parent")).expect("programs dir");
        std::fs::write(&shortcut, b"link").expect("shortcut");

        let marker = marker_path(&data_dir);
        std::fs::create_dir_all(marker.parent().expect("parent")).expect("app dir");
        std::fs::write(
            &marker,
            executable_signature(std::path::Path::new(r"C:\old\scico.exe")),
        )
        .expect("old marker");

        let old = std::path::Path::new(r"C:\old\scico.exe");
        assert!(shortcut_is_current(&data_dir, old));
        assert!(!shortcut_is_current(
            &data_dir,
            std::path::Path::new(r"C:\new\scico.exe")
        ));

        std::fs::remove_file(&shortcut).expect("remove shortcut");
        assert!(!shortcut_is_current(&data_dir, old));

        std::fs::remove_dir_all(&data_dir).expect("clean up");
    }
}
