//! eeemail's desktop shell.
//!
//! Deliberately thin. All behaviour lives in `deltachat-jsonrpc`, which the CLI
//! and any future client also use; the shell's job is to run that API
//! in-process, hand the frontend a way to talk to it, and forward what comes
//! back.
//!
//! # One transport, not a command per method
//!
//! The RPC surface is a couple of hundred methods and grows every phase.
//! Mirroring each as a `#[tauri::command]` would mean writing every signature
//! three times -- Rust, the handler list, TypeScript -- when
//! `deltachat-jsonrpc` already generates a type-checked TypeScript client. So
//! the shell exposes a single JSON-RPC pipe and the frontend speaks the
//! protocol over it.
//!
//! This mirrors how `deltachat-rpc-server` works, with Tauri's IPC in place of
//! stdin/stdout: requests go in through [`rpc_send`], and *everything* coming
//! back -- responses and engine events alike -- is emitted as an `rpc-message`
//! event. Responses are not returned from the command, because the JSON-RPC
//! session delivers them asynchronously on its outbound channel.
//!
//! # The three things that are not the pipe
//!
//! [`stage_attachment`] exists because a `File` in the renderer has no
//! filesystem path and core carries attachments by path. [`first_run_pending`]
//! and [`acknowledge_first_run`] exist because the disclosure in
//! `docs/adr/0023-first-launch-disclosure.md` has to be shown *before* an
//! account exists, and every per-account setting lives behind the pipe in a
//! database that has not been created yet.
//!
//! # Untrusted content
//!
//! The reading pane renders mail from strangers. The window's CSP allows no
//! remote origins at all, so nothing in a message can reach the network,
//! tracking pixels included. HTML mail is rendered in a sandboxed frame with a
//! `null` origin, never in the app document. See `docs/adr/0013-desktop-ui.md`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use deltachat::accounts::Accounts;
use deltachat_jsonrpc::api::CommandApi;
use futures_lite::stream::StreamExt as _;
use tauri::Emitter;
use tokio::sync::RwLock;
use yerpc::{RpcClient, RpcSession};

/// The JSON-RPC session, shared with every IPC call.
struct AppState {
    session: RpcSession<CommandApi>,
}

/// Feeds one JSON-RPC request into the session.
///
/// Returns nothing: the response arrives on the `rpc-message` event stream like
/// everything else, so the frontend has one path to handle rather than two.
#[tauri::command]
async fn rpc_send(state: tauri::State<'_, Arc<AppState>>, request: String) -> Result<(), String> {
    let session = state.session.clone();
    // Spawned rather than awaited: a long-running call such as a fetch must not
    // block the IPC thread and stall every other request behind it.
    tauri::async_runtime::spawn(async move {
        session.handle_incoming(&request).await;
    });
    Ok(())
}

/// Writes a picked attachment somewhere the engine can read it, and returns
/// the path.
///
/// A `File` in the renderer has no filesystem path -- the browser security
/// model does not give it one -- and the engine takes a path, because that is
/// how core carries attachments. So the bytes come across the IPC boundary once
/// and land in a staging directory beside the accounts.
///
/// The file name is reduced to its last component before use: a name is
/// attacker-influenced whenever the user forwards something, and `../` in it
/// would otherwise write outside the staging directory.
#[tauri::command]
async fn stage_attachment(name: String, bytes: Vec<u8>) -> Result<String, String> {
    let dir = accounts_dir()
        .map_err(|err| err.to_string())?
        .join("staging");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;

    let safe = std::path::Path::new(&name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty() && n != "." && n != "..")
        .unwrap_or_else(|| "attachment".to_string());
    // Prefixed with a nanosecond timestamp so two files with the same name in
    // one session do not overwrite each other mid-compose.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = dir.join(format!("{stamp}-{safe}"));

    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(path.to_string_lossy().to_string())
}

/// Whether the first-launch disclosure still has to be shown.
#[tauri::command]
async fn first_run_pending() -> Result<bool, String> {
    match first_run_marker() {
        Ok(path) => Ok(!path.exists()),
        // Err on the side of showing it. A marker we cannot read is not
        // evidence that anybody read the dialog.
        Err(_) => Ok(true),
    }
}

/// Records that the user acknowledged the disclosure.
#[tauri::command]
async fn acknowledge_first_run() -> Result<(), String> {
    let path = first_run_marker().map_err(|err| err.to_string())?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    tokio::fs::write(&path, b"1\n")
        .await
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(())
}

fn main() -> Result<()> {
    let result = tauri::async_runtime::block_on(async { run().await });
    if let Err(err) = &result {
        report_fatal(err);
    }
    result
}

/// Says why the app is not going to appear.
///
/// On Windows a release build is `windows_subsystem = "windows"`, so it has no
/// console: the error returned from `main` and every `eprintln!` go to a stderr
/// that does not exist. That is how the `accounts_dir()` bug survived eight
/// releases -- the app simply did not start, and there was nothing to report.
/// A message box turns the next one into a bug report on the first day.
fn report_fatal(err: &anyhow::Error) {
    eprintln!("eeemail cannot start: {err:#}");

    #[cfg(target_os = "windows")]
    {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt as _;

        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBoxW(
                hwnd: *mut c_void,
                text: *const u16,
                caption: *const u16,
                ty: u32,
            ) -> i32;
        }

        /// A NUL-terminated UTF-16 string, which is what the Win32 `W` entry
        /// points take.
        fn wide(s: &str) -> Vec<u16> {
            std::ffi::OsStr::new(s)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }

        const MB_OK: u32 = 0x0000_0000;
        const MB_ICONERROR: u32 = 0x0000_0010;

        let text = wide(&format!("eeemail cannot start.\n\n{err:#}"));
        let caption = wide("eeemail");
        // Nothing to do if this fails: we are already on the way out.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

async fn run() -> Result<()> {
    let dir = accounts_dir()?;
    let accounts = Accounts::new(dir.clone(), true)
        .await
        .with_context(|| format!("cannot open accounts at {}", dir.display()))?;
    let accounts = Arc::new(RwLock::new(accounts));

    let api = CommandApi::from_arc(accounts.clone()).await;
    let (client, mut outbound) = RpcClient::new();
    let session = RpcSession::new(client, api);
    let state = Arc::new(AppState { session });

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            let handle = app.handle().clone();
            // Everything the engine has to say -- responses, new mail, delivery
            // receipts, connectivity -- comes out here. Pushed, never polled, so
            // the UI never has to guess an interval.
            tauri::async_runtime::spawn(async move {
                while let Some(message) = outbound.next().await {
                    match serde_json::to_string(&message) {
                        Ok(json) => {
                            let _ = handle.emit("rpc-message", json);
                        }
                        // Dropping one malformed message is better than tearing
                        // down the pipe every later message depends on.
                        Err(err) => eprintln!("cannot serialize RPC message: {err:#}"),
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            rpc_send,
            stage_attachment,
            first_run_pending,
            acknowledge_first_run
        ])
        .run(tauri::generate_context!())
        .context("cannot run the desktop shell")?;
    Ok(())
}

/// The file that makes an unzipped copy portable.
///
/// It ships inside the release archive and nowhere else, so the same executable
/// is portable when it was unzipped and ordinary when it was installed. An
/// installed copy therefore cannot pick up a portable profile, and the two can
/// sit on one machine without meeting.
const PORTABLE_MARKER: &str = "eeemail-portable";

/// Whose convention to follow.
///
/// A value rather than a `cfg`, so the Windows rule can be *tested* from a
/// Linux machine. The rule that shipped broken through eight releases is
/// precisely the one no test on the build machine could reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    MacOs,
    Other,
}

fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Other
    }
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The directory the running executable sits in, if it is marked portable.
fn portable_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join(PORTABLE_MARKER)
        .is_file()
        .then(|| dir.to_path_buf())
}

/// Where eeemail keeps everything: the accounts, the attachment staging area,
/// and the first-run marker.
///
/// Per platform, because this is the difference between an installed app that
/// starts and one that exits with an error. The Unix-only version of this read
/// `XDG_DATA_HOME` then `HOME` and gave up if it found neither -- which is the
/// ordinary state of a Windows session, so every Windows build failed on launch
/// while the release workflow went on building one.
///
/// A portable copy overrides all of that and keeps the profile beside the
/// executable, so the archive can be unzipped, run, tested and deleted without
/// leaving anything behind. See `docs/adr/0024-portable-archives.md`.
fn data_dir() -> Result<PathBuf> {
    data_dir_from(current_platform(), portable_root().as_deref(), env_var)
}

/// The rule itself, with everything it reads passed in.
fn data_dir_from(
    platform: Platform,
    portable_root: Option<&Path>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf> {
    if let Some(root) = portable_root {
        return Ok(root.join("data"));
    }
    // An empty variable is an absent one. `XDG_DATA_HOME=` is how a shell
    // spells "unset" often enough that reading it as a path -- which puts the
    // whole mailbox at `/eeemail` -- is worth one line here. The filter belongs
    // in this function rather than in the caller that reads the real
    // environment, because this is the function with tests.
    let env = |name: &str| env(name).filter(|value| !value.is_empty());
    let base = match platform {
        Platform::Windows => PathBuf::from(env("APPDATA").context("APPDATA is not set")?),
        Platform::MacOs => PathBuf::from(env("HOME").context("HOME is not set")?)
            .join("Library")
            .join("Application Support"),
        Platform::Other => match env("XDG_DATA_HOME") {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(env("HOME").context("neither XDG_DATA_HOME nor HOME is set")?)
                .join(".local")
                .join("share"),
        },
    };
    Ok(base.join("eeemail"))
}

/// Where accounts live.
///
/// `EEEMAIL_ACCOUNTS_DIR` overrides it, so a developer or an integration test
/// can point the app at a scratch directory rather than the real profile.
fn accounts_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("EEEMAIL_ACCOUNTS_DIR") {
        return Ok(dir.into());
    }
    Ok(data_dir()?.join("accounts"))
}

/// The marker that says the user has seen what this software is.
///
/// A file rather than a config value, because the disclosure has to be shown
/// *before* an account exists and `Config` is per-account. It sits beside the
/// accounts rather than inside one for the same reason: it outlives any single
/// account, including deleting them all and starting over -- at which point the
/// person at the keyboard has already read it.
fn first_run_marker() -> Result<PathBuf> {
    Ok(data_dir()?.join("first-run-acknowledged"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug that made v0.3.0 unusable, asserted directly.
    ///
    /// `desktop/src-tauri/capabilities/` did not exist. Tauri resolves its ACL
    /// by globbing that directory at build time, so it compiled to `{}`: the
    /// four commands registered in `generate_handler!` are not ACL-checked and
    /// worked, but `listen()` is the core *event plugin* and was refused. The
    /// frontend attaches the `rpc-message` stream before its first request, so
    /// nothing resolved -- no accounts, no mail, no events, on every platform.
    ///
    /// This reads the same compiled authority the running app does, so it fails
    /// if the capability file is deleted, renamed, retargeted at a window label
    /// the config does not define, or narrowed to drop `listen`. Nothing else
    /// here exercises the IPC path: the screenshots are a browser-only demo
    /// build and `e2e-pass.py` drives a binary the app does not use.
    #[test]
    fn test_the_frontend_is_allowed_to_hear_the_engine() {
        let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let authority = context.runtime_authority_mut();
        for command in ["plugin:event|listen", "plugin:event|unlisten"] {
            assert!(
                authority
                    .resolve_access(command, "main", "main", &tauri::ipc::Origin::Local)
                    .is_some_and(|resolved| !resolved.is_empty()),
                "{command} is refused for the `main` window; the app cannot receive engine events"
            );
        }
    }

    /// The window the capability targets has to be the window the config
    /// creates. Tauri defaults an unlabelled window to `main`, so this held by
    /// accident until `tauri.conf.json` said so.
    #[test]
    fn test_the_capability_targets_a_window_that_exists() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let labels: Vec<_> = context
            .config()
            .app
            .windows
            .iter()
            .map(|window| window.label.as_str())
            .collect();
        assert!(labels.contains(&"main"), "windows are {labels:?}");
    }

    /// An environment with nothing in it.
    fn empty(_: &str) -> Option<String> {
        None
    }

    /// An environment built from pairs, so each test says exactly what it sets.
    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn test_windows_uses_appdata() {
        let dir = data_dir_from(
            Platform::Windows,
            None,
            env(&[("APPDATA", r"C:\Users\x\AppData\Roaming")]),
        )
        .unwrap();
        assert_eq!(
            dir,
            PathBuf::from(r"C:\Users\x\AppData\Roaming").join("eeemail")
        );
    }

    /// The bug v0.3.0 was cut to fix: a Windows session has neither, and the
    /// old code errored. It must not start reading them again.
    #[test]
    fn test_windows_never_reads_home_or_xdg() {
        let dir = data_dir_from(
            Platform::Windows,
            None,
            env(&[
                ("APPDATA", r"C:\AppData"),
                ("HOME", "/home/nobody"),
                ("XDG_DATA_HOME", "/xdg"),
            ]),
        )
        .unwrap();
        assert_eq!(dir, PathBuf::from(r"C:\AppData").join("eeemail"));
    }

    /// An error, not a panic, and one that names what is missing -- because on
    /// Windows this message is the only thing the user will ever see.
    #[test]
    fn test_windows_without_appdata_is_an_error() {
        let err = data_dir_from(Platform::Windows, None, empty).unwrap_err();
        assert!(err.to_string().contains("APPDATA"), "{err:#}");
    }

    #[test]
    fn test_linux_prefers_xdg_data_home() {
        let dir = data_dir_from(
            Platform::Other,
            None,
            env(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/home/x")]),
        )
        .unwrap();
        assert_eq!(dir, PathBuf::from("/xdg/eeemail"));
    }

    #[test]
    fn test_linux_falls_back_to_home() {
        let dir = data_dir_from(Platform::Other, None, env(&[("HOME", "/home/x")])).unwrap();
        assert_eq!(dir, PathBuf::from("/home/x/.local/share/eeemail"));
    }

    /// `XDG_DATA_HOME=` is how a shell spells "unset". Reading it as a path
    /// puts the whole mailbox at `/eeemail`.
    #[test]
    fn test_an_empty_xdg_data_home_is_not_a_path() {
        let dir = data_dir_from(
            Platform::Other,
            None,
            env(&[("XDG_DATA_HOME", ""), ("HOME", "/home/x")]),
        )
        .unwrap();
        assert_eq!(dir, PathBuf::from("/home/x/.local/share/eeemail"));
    }

    #[test]
    fn test_macos_uses_application_support() {
        let dir = data_dir_from(Platform::MacOs, None, env(&[("HOME", "/Users/x")])).unwrap();
        assert_eq!(
            dir,
            PathBuf::from("/Users/x/Library/Application Support/eeemail")
        );
    }

    /// The portable rule outranks every platform convention, on every platform,
    /// and never consults the environment -- an unzipped copy must not find a
    /// profile an installed copy is using.
    #[test]
    fn test_a_portable_copy_keeps_its_profile_beside_the_executable() {
        let root = Path::new("/media/usb/eeemail");
        for platform in [Platform::Windows, Platform::MacOs, Platform::Other] {
            let dir = data_dir_from(
                platform,
                Some(root),
                env(&[
                    ("APPDATA", r"C:\AppData"),
                    ("HOME", "/home/x"),
                    ("XDG_DATA_HOME", "/xdg"),
                ]),
            )
            .unwrap();
            assert_eq!(dir, root.join("data"), "{platform:?}");
        }
    }

    /// A portable copy needs no environment at all, which is the point: it runs
    /// on a machine where nothing has been set up for it.
    #[test]
    fn test_a_portable_copy_needs_no_environment() {
        let dir = data_dir_from(Platform::Windows, Some(Path::new("/usb")), empty).unwrap();
        assert_eq!(dir, PathBuf::from("/usb/data"));
    }
}
