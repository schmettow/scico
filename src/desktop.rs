//! Desktop integration for Linux launchers.
//!
//! GNOME on Wayland maps a window to an application through the Wayland
//! `app_id` and the matching `.desktop` entry. The window icon is an X11
//! concept, so a Wayland session shows a generic mark in the dash unless an
//! entry and its icon are installed. Scico installs both on first launch and
//! refreshes them whenever the executable path changes.
//!
//! Everything is written below `XDG_DATA_HOME` (usually `~/.local/share`), so
//! no root rights are needed and uninstalling is a matter of deleting the
//! three files.

use std::fs;
use std::io;
use std::path::Path;

use crate::APP_ID;

const ICON_PNG: &[u8] = include_bytes!("../assets/logo/Scico-square.png");
const ICON_SVG: &[u8] = include_bytes!("../assets/logo/Scico square.svg");

/// Install or refresh the desktop entry and icons.
///
/// Failures are reported but never stop the editor from starting: a missing
/// launcher entry only costs the shell icon.
pub fn install() {
    let Some(data_home) = dirs::data_dir() else {
        return;
    };
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    if let Err(error) = install_to(&data_home, &executable) {
        eprintln!("failed to install the Scico desktop entry: {error}");
    }
}

/// Write the launcher entry, icon, and SVG below `data_home`.
fn install_to(data_home: &Path, executable: &Path) -> io::Result<()> {
    let icon = data_home.join(format!("icons/hicolor/256x256/apps/{APP_ID}.png"));
    let scalable = data_home.join(format!("icons/hicolor/scalable/apps/{APP_ID}.svg"));
    let entry = data_home.join(format!("applications/{APP_ID}.desktop"));

    write_if_changed(&icon, ICON_PNG)?;
    write_if_changed(&scalable, ICON_SVG)?;
    write_if_changed(&entry, desktop_entry(executable, &icon).as_bytes())?;
    Ok(())
}

fn desktop_entry(executable: &Path, icon: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=Scico\n\
         GenericName=Scientific Markdown Editor\n\
         Comment=Scientific Markdown writing with integrated RAG co-writing and reference management\n\
         Exec={} %U\n\
         Icon={}\n\
         Terminal=false\n\
         Categories=Office;TextEditor;\n\
         MimeType=text/markdown;\n\
         Keywords=markdown;editor;research;writing;RAG;references;\n\
         StartupNotify=true\n\
         StartupWMClass={APP_ID}\n",
        quote_exec(executable),
        icon.display(),
    )
}

/// Quote an executable for the `Exec` key.
///
/// The desktop-entry specification reserves `"`, `` ` ``, `$`, and `\` inside
/// double quotes, and expands `%` field codes, so a literal `%` becomes `%%`.
fn quote_exec(executable: &Path) -> String {
    let mut quoted = String::with_capacity(executable.as_os_str().len() + 2);
    quoted.push('"');
    for character in executable.to_string_lossy().chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
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

    use super::{desktop_entry, install_to, quote_exec};
    use crate::APP_ID;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn scratch_dir() -> PathBuf {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("scico-desktop-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn the_entry_names_the_wayland_app_id_and_x11_wm_class() {
        let entry = desktop_entry(
            std::path::Path::new("/usr/local/bin/scico"),
            std::path::Path::new("/home/me/.local/share/icons/hicolor/256x256/apps/scico.png"),
        );

        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("Name=Scico\n"));
        assert!(entry.contains(&format!("StartupWMClass={APP_ID}\n")));
        assert!(entry.contains("Exec=\"/usr/local/bin/scico\" %U\n"));
        assert!(
            entry.contains("Icon=/home/me/.local/share/icons/hicolor/256x256/apps/scico.png\n")
        );
    }

    #[test]
    fn an_executable_with_spaces_and_reserved_characters_stays_one_argument() {
        assert_eq!(
            quote_exec(std::path::Path::new("/home/me/My Apps/scico")),
            "\"/home/me/My Apps/scico\""
        );
        assert_eq!(
            quote_exec(std::path::Path::new("/tmp/a\"b$c`d\\e%f")),
            "\"/tmp/a\\\"b\\$c\\`d\\\\e%%f\""
        );
    }

    #[test]
    fn installation_writes_the_entry_and_both_icons_idempotently() {
        let data_home = scratch_dir();
        let executable = std::path::Path::new("/usr/local/bin/scico");

        install_to(&data_home, executable).expect("first install");
        install_to(&data_home, executable).expect("second install");

        let entry = std::fs::read_to_string(data_home.join("applications/scico.desktop"))
            .expect("desktop entry");
        assert!(entry.contains("Exec=\"/usr/local/bin/scico\" %U"));
        assert!(entry.contains("StartupWMClass=scico"));

        let png = data_home.join("icons/hicolor/256x256/apps/scico.png");
        assert!(!std::fs::read(&png).expect("png icon").is_empty());
        assert!(
            entry.contains(&format!("Icon={}\n", png.display())),
            "the entry points at the installed icon: {entry}"
        );

        let svg = data_home.join("icons/hicolor/scalable/apps/scico.svg");
        assert!(!std::fs::read(&svg).expect("svg icon").is_empty());

        std::fs::remove_dir_all(&data_home).expect("clean up");
    }
}
