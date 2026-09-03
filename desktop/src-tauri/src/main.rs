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

use std::path::PathBuf;
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
    tauri::async_runtime::block_on(async { run().await })
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

/// Where eeemail keeps everything: the accounts, the attachment staging area,
/// and the first-run marker.
///
/// Per platform, because this is the difference between an installed app that
/// starts and one that exits with an error. The Unix-only version of this read
/// `XDG_DATA_HOME` then `HOME` and gave up if it found neither -- which is the
/// ordinary state of a Windows session, so every Windows build failed on launch
/// while the release workflow went on building one.
fn data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = PathBuf::from(std::env::var("APPDATA").context("APPDATA is not set")?);
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(std::env::var("HOME").context("HOME is not set")?)
        .join("Library")
        .join("Application Support");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(dir)
    } else {
        let home = std::env::var("HOME").context("neither XDG_DATA_HOME nor HOME is set")?;
        PathBuf::from(home).join(".local").join("share")
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
