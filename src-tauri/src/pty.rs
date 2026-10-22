//! Terminal sessions (D1): a real PTY per session running `$SHELL` in the
//! workspace folder. Output goes to the window as raw bytes over a channel;
//! sessions keep running while hidden and die with their window.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tauri::ipc::{Channel, Response};

struct Session {
    window: String,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    cwd: PathBuf,
}

#[derive(Default)]
pub struct Terminals {
    next: AtomicU64,
    map: Mutex<HashMap<u64, Session>>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { cols: cols.max(2), rows: rows.max(1), pixel_width: 0, pixel_height: 0 }
}

impl Terminals {
    /// Start `$SHELL` in `cwd`. Output arrives on `output`; when the shell
    /// exits, an empty message follows.
    pub fn open(&self, window: &str, cwd: &Path, cols: u16, rows: u16, output: Channel<Response>) -> Result<u64, String> {
        let pair = native_pty_system().openpty(size(cols, rows)).map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "maido");
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        // The slave end belongs to the shell now; keeping it open here would
        // stop the reader from seeing the shell exit.
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if output.send(Response::new(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = output.send(Response::new(Vec::new()));
        });
        let session = Session { window: window.to_string(), master: pair.master, writer, child, cwd: cwd.to_path_buf() };
        self.map.lock().unwrap().insert(id, session);
        Ok(id)
    }

    /// Keystrokes and pastes from the terminal. Only the user's own input
    /// reaches here; documents never do (F5).
    pub fn write(&self, window: &str, id: u64, data: &[u8]) -> Result<(), String> {
        let mut map = self.map.lock().unwrap();
        let s = map.get_mut(&id).filter(|s| s.window == window).ok_or("no such terminal")?;
        s.writer.write_all(data).and_then(|_| s.writer.flush()).map_err(|e| e.to_string())
    }

    pub fn resize(&self, window: &str, id: u64, cols: u16, rows: u16) -> Result<(), String> {
        let map = self.map.lock().unwrap();
        let s = map.get(&id).filter(|s| s.window == window).ok_or("no such terminal")?;
        s.master.resize(size(cols, rows)).map_err(|e| e.to_string())
    }

    pub fn close(&self, window: &str, id: u64) {
        let mut map = self.map.lock().unwrap();
        if map.get(&id).is_some_and(|s| s.window == window) {
            if let Some(mut s) = map.remove(&id) {
                let _ = s.child.kill();
            }
        }
    }

    /// The window went away: its sessions go with it.
    pub fn close_window(&self, window: &str) {
        let mut map = self.map.lock().unwrap();
        let ids: Vec<u64> = map.iter().filter(|(_, s)| s.window == window).map(|(id, _)| *id).collect();
        for id in ids {
            if let Some(mut s) = map.remove(&id) {
                let _ = s.child.kill();
            }
        }
    }

    /// The folder relative paths in the session's output are read against:
    /// the foreground program's current folder where the system tells us
    /// (Linux), else the folder the session started in.
    pub fn cwd(&self, window: &str, id: u64) -> Option<PathBuf> {
        let map = self.map.lock().unwrap();
        let s = map.get(&id).filter(|s| s.window == window)?;
        #[cfg(target_os = "linux")]
        if let Some(pid) = s.master.process_group_leader() {
            if let Ok(dir) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
                return Some(dir);
            }
        }
        Some(s.cwd.clone())
    }
}

/// A path as printed by a program: `docs/a.md`, `./a.md:12`, `/abs/a.md:12:3`.
/// Returns the path text and the line and column, if any.
pub fn split_location(text: &str) -> (&str, Option<u32>, Option<u32>) {
    fn number(s: &str) -> Option<(&str, u32)> {
        let (rest, n) = s.rsplit_once(':')?;
        (!rest.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| (rest, n.parse().ok())).and_then(|(r, n)| Some((r, n?)))
    }
    match number(text) {
        None => (text, None, None),
        Some((rest, n)) => match number(rest) {
            Some((path, line)) => (path, Some(line), Some(n)),
            None => (rest, Some(n), None),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations() {
        assert_eq!(split_location("docs/a.md"), ("docs/a.md", None, None));
        assert_eq!(split_location("docs/a.md:12"), ("docs/a.md", Some(12), None));
        assert_eq!(split_location("/x/a.md:12:3"), ("/x/a.md", Some(12), Some(3)));
        assert_eq!(split_location("a:b.md:7"), ("a:b.md", Some(7), None));
        assert_eq!(split_location("a.md:"), ("a.md:", None, None));
    }

    #[test]
    fn shell_runs_and_exits() {
        let sessions = Terminals::default();
        let dir = std::env::temp_dir();
        // No window here: a channel whose messages go nowhere.
        let out = Channel::new(|_| Ok(()));
        let id = sessions.open("w", &dir, 80, 24, out).unwrap();
        sessions.write("w", id, b"exit\r").unwrap();
        assert!(sessions.write("other", id, b"x").is_err());
        assert!(sessions.cwd("w", id).is_some());
        sessions.close_window("w");
        assert!(sessions.write("w", id, b"x").is_err());
    }
}
