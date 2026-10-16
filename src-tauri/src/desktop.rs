//! Linux desktop integration for unpackaged runs (`just dev`, AppImage).
//!
//! GNOME and KDE on Wayland take the dock/taskbar icon from the desktop entry whose name matches
//! the window's app ID (`enableGTKAppId` in tauri.conf.json), not from the window itself.
//! Flatpak and system packages ship that entry; a bare binary or an AppImage does not, so it
//! would show a generic icon. In that case we install the entry and icon into `~/.local/share`
//! before the window opens.

use std::path::{Path, PathBuf};
use std::{env, fs, io};

const APP_ID: &str = "io.github.mindulabs.Quarry";
const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");
const ICON_SVG: &[u8] = include_bytes!("../icons/icon.svg");

pub fn integrate() {
    if env::var_os("FLATPAK_ID").is_some() || installed_by_package() {
        return;
    }
    // A package manager put us in /usr and owns the desktop entry.
    if env::var_os("APPIMAGE").is_none()
        && env::current_exe().is_ok_and(|exe| exe.starts_with("/usr"))
    {
        return;
    }
    if let Err(err) = install_user_entry() {
        tracing::warn!(%err, "desktop integration");
    }
}

/// A system package (`.deb`, distro package) already provides the entry.
fn installed_by_package() -> bool {
    let dirs = env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.split(':').filter(|dir| !dir.is_empty()).any(|dir| {
        Path::new(dir)
            .join("applications")
            .join(format!("{APP_ID}.desktop"))
            .is_file()
    })
}

fn data_home() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}

fn install_user_entry() -> io::Result<()> {
    let Some(data) = data_home() else {
        return Ok(());
    };
    // An AppImage runs from a temporary mount; launch the image itself.
    let exec = match env::var_os("APPIMAGE") {
        Some(image) => PathBuf::from(image),
        None => env::current_exe()?,
    };
    let entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Quarry\n\
         GenericName=Database Client\n\
         Comment=Query and edit your databases\n\
         Exec=\"{}\"\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Development;Database;\n\
         StartupWMClass={APP_ID}\n\
         StartupNotify=true\n",
        exec.display()
    );

    write_if_changed(
        &data
            .join("icons/hicolor/512x512/apps")
            .join(format!("{APP_ID}.png")),
        ICON_PNG,
    )?;
    write_if_changed(
        &data
            .join("icons/hicolor/scalable/apps")
            .join(format!("{APP_ID}.svg")),
        ICON_SVG,
    )?;
    write_if_changed(
        &data.join("applications").join(format!("{APP_ID}.desktop")),
        entry.as_bytes(),
    )
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}
