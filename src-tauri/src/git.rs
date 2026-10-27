//! The Git panel (G1): status, commit and sync of the workspace folder,
//! through the system's `git` (your keys, credential helpers and config).
//! Only the workspace's own folder, and only what lies inside it.

use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

#[derive(Serialize, Default)]
pub struct Status {
    /// The folder is inside a Git repository.
    repo: bool,
    branch: String,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    /// The repository has a remote to sync with.
    remote: bool,
    changes: Vec<Change>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Change {
    /// Relative to the workspace folder.
    path: String,
    /// `git status --porcelain`'s two letters: staged, then unstaged ("??" untracked).
    code: String,
}

/// Run `git` in `dir`. Never prompts (a password asked for fails instead,
/// with git's message), never runs a repository's fsmonitor command.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotePath=false", "-c", "color.ui=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "git is not installed".to_string() } else { format!("git: {e}") })?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let message = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
    Err(message.to_string())
}

pub fn status(dir: &Path) -> Result<Status, String> {
    // Where the workspace sits in the repository ("" at its top, "docs/" below).
    let Ok(prefix) = git(dir, &["rev-parse", "--show-prefix"]) else { return Ok(Status::default()) };
    let prefix = prefix.trim();
    let out = git(dir, &["--no-optional-locks", "status", "--porcelain=v1", "--branch", "--untracked-files=all", "-z", "--", "."])?;
    let mut status = parse_status(&out, prefix);
    status.remote = !git(dir, &["remote"])?.trim().is_empty();
    Ok(status)
}

fn parse_status(out: &str, prefix: &str) -> Status {
    let mut status = Status { repo: true, ..Status::default() };
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    while let Some(f) = fields.next() {
        if let Some(head) = f.strip_prefix("## ") {
            parse_branch(head, &mut status);
            continue;
        }
        let (code, path) = (f.get(..2).unwrap_or(""), f.get(3..).unwrap_or(""));
        // A rename or copy is followed by the path it came from.
        if code.contains('R') || code.contains('C') {
            fields.next();
        }
        let path = path.strip_prefix(prefix).unwrap_or(path);
        status.changes.push(Change { path: path.to_string(), code: code.to_string() });
    }
    status
}

/// `main...origin/main [ahead 1, behind 2]`, `main`, `No commits yet on main`, `HEAD (no branch)`.
fn parse_branch(head: &str, status: &mut Status) {
    let head = head.strip_prefix("No commits yet on ").unwrap_or(head);
    let (names, counts) = head.split_once(" [").unwrap_or((head, ""));
    match names.split_once("...") {
        Some((branch, upstream)) => {
            status.branch = branch.to_string();
            status.upstream = Some(upstream.to_string());
        }
        None => status.branch = names.to_string(),
    }
    for part in counts.trim_end_matches(']').split(", ") {
        let n = |s: &str| s.parse().unwrap_or(0);
        if let Some(a) = part.strip_prefix("ahead ") {
            status.ahead = n(a);
        } else if let Some(b) = part.strip_prefix("behind ") {
            status.behind = n(b);
        }
    }
}

/// Commit everything changed inside the workspace folder. Returns false when
/// there was nothing to commit.
pub fn commit(dir: &Path, message: &str) -> Result<bool, String> {
    git(dir, &["add", "--all", "--", "."])?;
    if git(dir, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(false);
    }
    git(dir, &["commit", "--quiet", "--message", message])?;
    Ok(true)
}

/// Sync: commit, then pull (rebasing your commits onto the remote's) and
/// push. Returns what was done, in a few words.
pub fn sync(dir: &Path, message: &str) -> Result<String, String> {
    let committed = commit(dir, message)?;
    let s = status(dir)?;
    if !s.remote {
        return Ok(if committed { "Committed. No remote to sync with.".into() } else { "Nothing to commit. No remote to sync with.".into() });
    }
    if s.upstream.is_some() {
        git(dir, &["pull", "--rebase", "--autostash", "--quiet"]).map_err(|e| {
            let stuck = dir.join(git(dir, &["rev-parse", "--git-dir"]).unwrap_or_default().trim()).join("rebase-merge").exists();
            if stuck { format!("{e}\n\nThe pull stopped on a conflict: resolve it in the terminal (git status), or undo it with git rebase --abort.") } else { e }
        })?;
        git(dir, &["push", "--quiet"])?;
    } else {
        // A branch never pushed: push it to the first remote and track it there.
        let remotes = git(dir, &["remote"])?;
        let remote = if remotes.lines().any(|r| r == "origin") { "origin" } else { remotes.lines().next().unwrap_or("origin") };
        git(dir, &["push", "--quiet", "--set-upstream", remote, "HEAD"])?;
    }
    Ok(if committed { "Committed and synced.".into() } else { "Synced.".into() })
}

pub fn init(dir: &Path) -> Result<(), String> {
    git(dir, &["init", "--quiet"]).map(|_| ())
}

/// A file's text at the last commit, for comparing; empty if it is new.
/// `path` is relative to the workspace folder and may not leave it.
pub fn head_text(dir: &Path, path: &str) -> Result<String, String> {
    if path.starts_with(['/', '@']) || path.split('/').any(|p| p == "..") {
        return Err(format!("{path} is outside the workspace"));
    }
    Ok(git(dir, &["show", &format!("HEAD:./{path}")]).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain() {
        let out = "## main...origin/main [ahead 2, behind 1]\0 M docs/a.md\0?? docs/new.md\0R  docs/b.md\0docs/old.md\0";
        let s = parse_status(out, "docs/");
        assert_eq!((s.branch.as_str(), s.upstream.as_deref(), s.ahead, s.behind), ("main", Some("origin/main"), 2, 1));
        assert_eq!(
            s.changes,
            [Change { path: "a.md".into(), code: " M".into() }, Change { path: "new.md".into(), code: "??".into() }, Change { path: "b.md".into(), code: "R ".into() }]
        );
        let s = parse_status("## No commits yet on main\0", "");
        assert_eq!((s.branch.as_str(), s.upstream, s.ahead), ("main", None, 0));
        assert_eq!(parse_status("## main...origin/main [behind 3]\0", "").behind, 3);
    }

    #[test]
    fn commit_in_a_repository() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("maido-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!status(&dir).unwrap().repo);
        init(&dir).unwrap();
        git(&dir, &["config", "user.email", "test@example.com"]).unwrap();
        git(&dir, &["config", "user.name", "Test"]).unwrap();
        std::fs::write(dir.join("a.md"), "one\n").unwrap();
        let s = status(&dir).unwrap();
        assert!(s.repo && !s.remote);
        assert_eq!(s.changes, [Change { path: "a.md".into(), code: "??".into() }]);
        assert_eq!(sync(&dir, "First").unwrap(), "Committed. No remote to sync with.");
        assert!(status(&dir).unwrap().changes.is_empty());
        std::fs::write(dir.join("a.md"), "two\n").unwrap();
        assert_eq!(head_text(&dir, "a.md").unwrap(), "one\n");
        assert_eq!(head_text(&dir, "new.md").unwrap(), "");
        assert!(head_text(&dir, "../x").is_err());
        assert!(commit(&dir, "Second").unwrap());
        assert!(!commit(&dir, "Nothing").unwrap());

        // A remote: the first sync pushes and tracks; a later one pulls what another clone pushed.
        let bare = dir.with_extension("remote");
        let other = dir.with_extension("other");
        for d in [&bare, &other] {
            let _ = std::fs::remove_dir_all(d);
        }
        git(&dir, &["init", "--quiet", "--bare", bare.to_str().unwrap()]).unwrap();
        git(&dir, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
        assert_eq!(sync(&dir, "x").unwrap(), "Synced.");
        assert!(status(&dir).unwrap().upstream.is_some());
        git(&dir, &["clone", "--quiet", bare.to_str().unwrap(), other.to_str().unwrap()]).unwrap();
        git(&other, &["config", "user.email", "test@example.com"]).unwrap();
        git(&other, &["config", "user.name", "Test"]).unwrap();
        std::fs::write(other.join("b.md"), "theirs\n").unwrap();
        assert_eq!(sync(&other, "Theirs").unwrap(), "Committed and synced.");
        std::fs::write(dir.join("c.md"), "mine\n").unwrap();
        assert_eq!(sync(&dir, "Mine").unwrap(), "Committed and synced.");
        assert_eq!(std::fs::read_to_string(dir.join("b.md")).unwrap(), "theirs\n");
        let s = status(&dir).unwrap();
        assert_eq!((s.ahead, s.behind, s.changes.len()), (0, 0, 0));
        for d in [&bare, &other, &dir] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
