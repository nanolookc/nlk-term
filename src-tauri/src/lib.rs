mod git;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    sync::Mutex,
};
use tauri::Emitter;

struct TerminalSession {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    shell: String,
}

struct TerminalState {
    sessions: Mutex<HashMap<String, TerminalSession>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenTerminalResponse {
    shell: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalDataEvent {
    tab_id: String,
    data: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalExitEvent {
    tab_id: String,
}

#[cfg(target_os = "windows")]
fn shell_details() -> (String, CommandBuilder) {
    let shell = "cmd.exe".to_string();
    let builder = CommandBuilder::new(shell.clone());
    (shell, builder)
}

#[cfg(not(target_os = "windows"))]
fn shell_details() -> (String, CommandBuilder) {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let mut builder = CommandBuilder::new(shell.clone());
    builder.env("TERM", "xterm-256color");
    builder.env("COLORTERM", "truecolor");
    builder.env("TERM_PROGRAM", "ghostty-web");
    builder.env("CLICOLOR", "1");
    (shell, builder)
}

#[tauri::command]
fn terminal_cwd(tab_id: String, state: tauri::State<TerminalState>) -> Result<Option<String>, String> {
    let sessions = state
        .sessions
        .lock()
        .map_err(|_| "failed to lock terminal sessions".to_string())?;

    let session = match sessions.get(&tab_id) {
        Some(session) => session,
        None => return Ok(None),
    };

    let pid = match session.child.process_id() {
        Some(pid) => pid,
        None => return Ok(None),
    };

    #[cfg(target_os = "linux")]
    {
        let link_path = PathBuf::from(format!("/proc/{pid}/cwd"));
        let cwd = std::fs::read_link(&link_path)
            .map_err(|error| format!("failed to read cwd link: {error}"))?;
        return Ok(Some(cwd.to_string_lossy().to_string()));
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        Ok(None)
    }
}

#[tauri::command]
fn open_terminal(
    tab_id: String,
    app: tauri::AppHandle,
    state: tauri::State<TerminalState>,
) -> Result<OpenTerminalResponse, String> {
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| "failed to lock terminal sessions".to_string())?;

    if let Some(session) = sessions.get(&tab_id) {
        return Ok(OpenTerminalResponse {
            shell: session.shell.clone(),
        });
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("failed to open pty: {error}"))?;

    let (shell, shell_command) = shell_details();

    let child = pair
        .slave
        .spawn_command(shell_command)
        .map_err(|error| format!("failed to spawn shell: {error}"))?;

    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("failed to clone pty reader: {error}"))?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("failed to get pty writer: {error}"))?;

    let app_handle = app.clone();
    let reader_tab_id = tab_id.clone();

    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    let data = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let _ = app_handle.emit(
                        "terminal-data",
                        TerminalDataEvent {
                            tab_id: reader_tab_id.clone(),
                            data,
                        },
                    );
                }
                Err(_) => break,
            }
        }

        let _ = app_handle.emit(
            "terminal-exit",
            TerminalExitEvent {
                tab_id: reader_tab_id,
            },
        );
    });

    sessions.insert(
        tab_id,
        TerminalSession {
            writer,
            master: pair.master,
            child,
            shell: shell.clone(),
        },
    );

    Ok(OpenTerminalResponse { shell })
}

#[tauri::command]
fn write_terminal(tab_id: String, data: String, state: tauri::State<TerminalState>) -> Result<(), String> {
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| "failed to lock terminal sessions".to_string())?;

    let session = sessions
        .get_mut(&tab_id)
        .ok_or_else(|| format!("terminal session not found: {tab_id}"))?;

    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|error| format!("failed to write to pty: {error}"))?;

    session
        .writer
        .flush()
        .map_err(|error| format!("failed to flush pty writer: {error}"))?;

    Ok(())
}

#[tauri::command]
fn resize_terminal(tab_id: String, cols: u16, rows: u16, state: tauri::State<TerminalState>) -> Result<(), String> {
    if cols == 0 || rows == 0 {
        return Ok(());
    }

    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| "failed to lock terminal sessions".to_string())?;

    if let Some(session) = sessions.get_mut(&tab_id) {
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("failed to resize pty: {error}"))?;
    }

    Ok(())
}

#[tauri::command]
fn close_terminal(tab_id: String, state: tauri::State<TerminalState>) -> Result<(), String> {
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| "failed to lock terminal sessions".to_string())?;

    if let Some(mut session) = sessions.remove(&tab_id) {
        let _ = session.child.kill();
        let _ = session.child.wait();
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(TerminalState {
            sessions: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            git::git_status,
            git::git_diff,
            git::git_stage,
            git::git_stage_all,
            git::git_unstage,
            git::git_commit,
            git::git_fetch,
            git::git_pull,
            git::git_push,
            git::git_branches,
            git::git_checkout,
            terminal_cwd,
            open_terminal,
            write_terminal,
            resize_terminal,
            close_terminal
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
