//! Restarting into an update that was just installed.
//!
//! An AppImage cannot use Tauri's restart as is. The running app was started by the old AppImage's
//! `AppRun`, which put paths inside the old mount into the environment, made that mount the
//! working directory, and passed on a descriptor of it. A child started from here inherits all
//! three, so the new version would read files from the old mount and keep it, and the old file,
//! alive until it quits. The child gets the environment without those paths, the directory the
//! user started from, and no inherited descriptors.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use tauri::AppHandle;

/// Set by the AppImage runtime on each start; the new start sets its own.
const RUNTIME_VARS: [&str; 4] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// Closes every descriptor above stderr, then runs the arguments. Bash, since `AppRun` needs it
/// too: it closes descriptors with more than one digit, which `sh` may not.
const CLOSE_FDS_THEN_EXEC: &str = r#"for fd in /proc/self/fd/*; do fd=${fd##*/}; [ "$fd" -gt 2 ] && eval "exec $fd>&-"; done 2>/dev/null; exec "$@""#;

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    let appimage = std::env::var_os("APPIMAGE");
    let appdir = std::env::var_os("APPDIR").filter(|dir| !dir.is_empty());
    let (Some(appimage), Some(appdir)) = (appimage, appdir) else {
        app.restart();
    };
    match relaunch_appimage(&appimage, &appdir) {
        Ok(()) => app.exit(0),
        Err(err) => {
            tracing::warn!(%err, "clean AppImage restart failed; restarting as is");
            app.restart();
        }
    }
}

fn relaunch_appimage(appimage: &OsStr, appdir: &OsStr) -> std::io::Result<()> {
    let appdir = appdir.as_bytes();
    let cwd = std::env::var_os("OWD")
        .filter(|dir| !is_inside(dir.as_bytes(), appdir))
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .or_else(std::env::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"));
    Command::new("bash")
        .arg("-c")
        .arg(CLOSE_FDS_THEN_EXEC)
        .arg("bash")
        .arg(appimage)
        .args(std::env::args_os().skip(1))
        .env_clear()
        .envs(launch_env(std::env::vars_os(), appdir))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .spawn()
        .map(drop)
}

/// The environment as it was before the AppImage at `appdir` started: its entries are taken out
/// of path lists, and a variable left with nothing is dropped. `appdir` is not empty.
fn launch_env(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
    appdir: &[u8],
) -> Vec<(OsString, OsString)> {
    vars.into_iter()
        .filter(|(key, _)| !RUNTIME_VARS.iter().any(|var| key == var))
        .filter_map(|(key, value)| {
            let bytes = value.as_bytes();
            if !bytes.windows(appdir.len()).any(|window| window == appdir) {
                return Some((key, value));
            }
            let kept: Vec<&[u8]> = bytes
                .split(|b| *b == b':')
                .filter(|entry| !entry.is_empty() && !is_inside(entry, appdir))
                .collect();
            (!kept.is_empty()).then(|| (key, OsString::from_vec(kept.join(&b':'))))
        })
        .collect()
}

/// `path` is `dir` or below it. `AppRun` writes some paths as `$APPDIR//usr`.
fn is_inside(path: &[u8], dir: &[u8]) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(b"/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPDIR: &[u8] = b"/tmp/.mount_QuarryAbc123";

    fn env(vars: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        vars.iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    #[test]
    fn appimage_entries_leave_path_lists() {
        let got = launch_env(
            env(&[
                (
                    "PATH",
                    "/tmp/.mount_QuarryAbc123/usr/bin/:/tmp/.mount_QuarryAbc123/bin/:/usr/bin:/bin",
                ),
                (
                    "XDG_DATA_DIRS",
                    "/tmp/.mount_QuarryAbc123/usr/share:/usr/share:/usr/local/share",
                ),
            ]),
            APPDIR,
        );
        assert_eq!(
            got,
            env(&[
                ("PATH", "/usr/bin:/bin"),
                ("XDG_DATA_DIRS", "/usr/share:/usr/local/share"),
            ])
        );
    }

    #[test]
    fn variables_only_appimage_set_are_dropped() {
        let got = launch_env(
            env(&[
                (
                    "GSETTINGS_SCHEMA_DIR",
                    "/tmp/.mount_QuarryAbc123//usr/share/glib-2.0/schemas",
                ),
                ("LD_LIBRARY_PATH", "/tmp/.mount_QuarryAbc123/usr/lib/:"),
                ("APPIMAGE", "/home/me/Quarry.AppImage"),
                ("OWD", "/home/me"),
                ("HOME", "/home/me"),
                ("GTK_THEME", "Adwaita:dark"),
            ]),
            APPDIR,
        );
        assert_eq!(
            got,
            env(&[("HOME", "/home/me"), ("GTK_THEME", "Adwaita:dark")])
        );
    }

    #[test]
    fn another_mount_with_the_same_prefix_is_kept() {
        let got = launch_env(
            env(&[("PATH", "/tmp/.mount_QuarryAbc1234/usr/bin:/usr/bin")]),
            APPDIR,
        );
        assert_eq!(
            got,
            env(&[("PATH", "/tmp/.mount_QuarryAbc1234/usr/bin:/usr/bin")])
        );
    }
}
