//! File actions from the Explorer's context menu (N4): new file or folder,
//! rename, move to the trash, copy or move. Every path stays inside the
//! workspace, and nothing is overwritten: a name that exists is refused, and
//! a paste onto one gets " copy" added.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::workspace::Workspace;

/// One file or folder name: not empty, no slashes, not `.` or `..`.
fn check_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(format!("\"{name}\" is not a valid name"));
    }
    Ok(name)
}

/// The folder `rel`, inside the workspace.
fn folder(ws: &Workspace, rel: &str) -> Result<PathBuf, String> {
    let dir = ws.resolve(rel)?;
    if !dir.is_dir() {
        return Err(format!("{rel} is not a folder"));
    }
    Ok(dir)
}

/// The file or folder `rel` itself: a symlink is the link, not what it points to.
fn item(ws: &Workspace, rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    let name = p.file_name().ok_or_else(|| format!("{rel}: not a file or folder in the workspace"))?;
    let path = folder(ws, &p.parent().unwrap_or(Path::new("")).to_string_lossy())?.join(name);
    path.symlink_metadata().map_err(|e| format!("{rel}: {e}"))?;
    Ok(path)
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn relative(ws: &Workspace, path: &Path) -> Result<String, String> {
    ws.relative(path).ok_or_else(|| format!("{} is outside the workspace", path.display()))
}

/// A new empty file, or a folder, named `name` in the folder `dir`.
pub fn create(ws: &Workspace, dir: &str, name: &str, is_folder: bool) -> Result<String, String> {
    let name = check_name(name)?;
    let path = folder(ws, dir)?.join(name);
    if exists(&path) {
        return Err(format!("{name} already exists"));
    }
    let made = if is_folder { std::fs::create_dir(&path) } else { std::fs::File::create_new(&path).map(drop) };
    made.map_err(|e| format!("{name}: {e}"))?;
    relative(ws, &path)
}

/// Give `rel` the name `name`, in the same folder. Returns its new path.
pub fn rename(ws: &Workspace, rel: &str, name: &str) -> Result<String, String> {
    let name = check_name(name)?;
    let from = item(ws, rel)?;
    let to = from.with_file_name(name);
    if to == from {
        return Ok(rel.to_string());
    }
    // A change of case only is the same file on some file systems.
    let same = from.file_name().map(|n| n.to_string_lossy().to_lowercase()) == Some(name.to_lowercase());
    if exists(&to) && !same {
        return Err(format!("{name} already exists"));
    }
    std::fs::rename(&from, &to).map_err(|e| format!("{rel}: {e}"))?;
    relative(ws, &to)
}

/// Move each of `paths` to the system trash, where it can be restored from.
pub fn trash(ws: &Workspace, paths: &[String]) -> Result<(), String> {
    for rel in paths {
        let path = item(ws, rel)?;
        trash::delete(&path).map_err(|e| format!("{rel}: {e}"))?;
    }
    Ok(())
}

/// Copy (or with `cut`, move) each of `paths` into the folder `dir`. Returns
/// where each one went.
pub fn paste(ws: &Workspace, paths: &[String], dir: &str, cut: bool) -> Result<Vec<String>, String> {
    let dest = folder(ws, dir)?;
    let mut out = Vec::new();
    for rel in paths {
        let from = item(ws, rel)?;
        if from.is_dir() && from.canonicalize().is_ok_and(|real| dest.starts_with(real)) {
            return Err(format!("{rel} cannot go inside itself"));
        }
        if cut && from.parent() == Some(dest.as_path()) {
            out.push(rel.clone());
            continue;
        }
        let name = from.file_name().ok_or("no file name")?;
        let to = free_name(&dest, name);
        if cut {
            // Across file systems a rename fails: copy, then remove.
            if std::fs::rename(&from, &to).is_err() {
                copy_all(&from, &to).map_err(|e| format!("{rel}: {e}"))?;
                remove_all(&from).map_err(|e| format!("{rel}: {e}"))?;
            }
        } else {
            copy_all(&from, &to).map_err(|e| format!("{rel}: {e}"))?;
        }
        out.push(relative(ws, &to)?);
    }
    Ok(out)
}

/// `name` in `dir`, or "name copy", "name copy 2"… if it is taken.
fn free_name(dir: &Path, name: &OsStr) -> PathBuf {
    let path = dir.join(name);
    if !exists(&path) {
        return path;
    }
    let p = Path::new(name);
    let stem = p.file_stem().unwrap_or(name).to_string_lossy();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{stem} copy{ext}") } else { format!("{stem} copy {n}{ext}") }))
        .find(|p| !exists(p))
        .unwrap()
}

/// Copy a file or a whole folder. Symlinks are copied as links, never followed
/// (they could lead out of the workspace).
fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = from.symlink_metadata()?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(std::fs::read_link(from)?, to)?;
        return Ok(());
    }
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(from, to).map(drop)
}

fn remove_all(path: &Path) -> std::io::Result<()> {
    if path.symlink_metadata()?.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rename_and_paste() {
        let dir = std::env::temp_dir().join(format!("maido-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        let ws = Workspace::new(dir.canonicalize().unwrap(), None);

        assert_eq!(create(&ws, "docs", "a.md", false).unwrap(), "docs/a.md");
        assert!(create(&ws, "docs", "a.md", false).is_err());
        assert!(create(&ws, "", "../out.md", false).is_err());
        assert!(create(&ws, "..", "out.md", false).is_err());
        assert_eq!(create(&ws, "", "notes", true).unwrap(), "notes");

        assert_eq!(rename(&ws, "docs/a.md", "b.md").unwrap(), "docs/b.md");
        assert!(rename(&ws, "docs/b.md", "../b.md").is_err());
        assert!(rename(&ws, "", "x").is_err());

        // Copies never overwrite.
        assert_eq!(paste(&ws, &["docs/b.md".into()], "docs", false).unwrap(), ["docs/b copy.md"]);
        assert_eq!(paste(&ws, &["docs/b.md".into()], "docs", false).unwrap(), ["docs/b copy 2.md"]);
        assert_eq!(paste(&ws, &["docs".into()], "notes", false).unwrap(), ["notes/docs"]);
        assert!(dir.join("notes/docs/b copy 2.md").is_file());
        assert!(paste(&ws, &["notes".into()], "notes/docs", true).is_err());

        assert_eq!(paste(&ws, &["docs/b.md".into()], "notes", true).unwrap(), ["notes/b.md"]);
        assert!(!dir.join("docs/b.md").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
