//! Quick open and full-text search (E1). Both walk the workspace the way the
//! tree does: `.gitignore` respected, hidden files left out.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::Lossy;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Utf32Str};
use serde::Serialize;

use crate::workspace::{is_doc, walker};

/// Quick open stops listing beyond this many files.
const MAX_FILES: usize = 200_000;
/// Search stops after this many matching lines in all, and this many per file.
const MAX_MATCHES: usize = 5_000;
const MAX_PER_FILE: usize = 200;
/// Longer lines are cut to this many characters around the first match.
const LINE_CHARS: usize = 240;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// Every file in the workspace, relative, sorted.
pub fn list_files(root: &Path) -> Vec<String> {
    let mut files: Vec<String> = walker(root)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| Some(e.path().strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/")))
        .take(MAX_FILES)
        .collect();
    files.sort();
    files
}

#[derive(Serialize, Debug)]
pub struct FileMatch {
    pub path: String,
    /// Positions of the matched characters (Unicode scalar values) in `path`.
    pub indices: Vec<u32>,
}

/// The best `limit` files for `query`, fuzzy-matched on their paths. Documents
/// rank above other files of equal score; an empty query lists documents.
pub fn quick_open(files: &[String], query: &str, limit: usize) -> Vec<FileMatch> {
    let query = query.trim();
    if query.is_empty() {
        return files.iter().filter(|f| is_doc(Path::new(f))).take(limit).map(|f| FileMatch { path: f.clone(), indices: vec![] }).collect();
    }
    let mut matcher = nucleo_matcher::Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, bool, &String)> = files
        .iter()
        .filter_map(|f| {
            let score = pattern.score(Utf32Str::new(f, &mut buf), &mut matcher)?;
            Some((score, is_doc(Path::new(f)), f))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.len().cmp(&b.2.len())).then(a.2.cmp(b.2)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, f)| {
            let mut indices = Vec::new();
            pattern.indices(Utf32Str::new(f, &mut buf), &mut matcher, &mut indices);
            indices.sort_unstable();
            indices.dedup();
            FileMatch { path: f.clone(), indices }
        })
        .collect()
}

#[derive(Serialize, Debug, Clone)]
pub struct LineMatch {
    pub line: u64,
    pub text: String,
    /// Matched ranges in `text`, in UTF-16 code units (JavaScript string offsets).
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SearchEvent {
    File { path: String, matches: Vec<LineMatch> },
    Done { files: usize, matches: usize, truncated: bool },
    Error { message: String },
}

/// Search every text file under `root` for `query` (plain text;
/// case-insensitive unless the query has capitals). Results go to
/// `send` file by file. Stops early when `generation` moves past `mine` (a
/// newer search started).
pub fn search(roots: &[(String, PathBuf)], query: &str, generation: &Arc<AtomicU64>, mine: u64, mut send: impl FnMut(SearchEvent)) {
    let matcher = match RegexMatcherBuilder::new().case_smart(true).fixed_strings(true).build(query) {
        Ok(m) => m,
        Err(e) => return send(SearchEvent::Error { message: e.to_string() }),
    };
    let mut searcher = SearcherBuilder::new().binary_detection(BinaryDetection::quit(0)).line_number(true).build();
    let (mut files, mut total, mut truncated) = (0, 0, false);
    'roots: for (prefix, root) in roots {
    for entry in walker(root).build().filter_map(Result::ok) {
        if generation.load(Ordering::Relaxed) != mine {
            return;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) || entry.metadata().map_or(true, |m| m.len() > MAX_FILE_BYTES) {
            continue;
        }
        let Some(rel) = entry.path().strip_prefix(root).ok().map(|p| p.to_string_lossy().replace('\\', "/")) else { continue };
        let rel = if prefix.is_empty() { rel } else { format!("{prefix}/{rel}") };
        let mut matches = Vec::new();
        let _ = searcher.search_path(
            &matcher,
            entry.path(),
            Lossy(|line, text| {
                let text = text.trim_end_matches(['\n', '\r']);
                let mut found = Vec::new();
                let _ = matcher.find_iter(text.as_bytes(), |m| {
                    found.push((m.start(), m.end()));
                    true
                });
                if !found.is_empty() {
                    matches.push(clip(line, text, &found));
                }
                Ok(matches.len() < MAX_PER_FILE && total + matches.len() < MAX_MATCHES)
            }),
        );
        if matches.is_empty() {
            continue;
        }
        files += 1;
        total += matches.len();
        if matches.len() >= MAX_PER_FILE || total >= MAX_MATCHES {
            truncated = true;
        }
        send(SearchEvent::File { path: rel, matches });
        if total >= MAX_MATCHES {
            break 'roots;
        }
    }
    }
    send(SearchEvent::Done { files, matches: total, truncated });
}

/// One matching line, cut to `LINE_CHARS` around its first match, with byte
/// ranges turned into UTF-16 offsets into the cut text.
fn clip(line: u64, text: &str, found: &[(usize, usize)]) -> LineMatch {
    let first = found[0].0;
    let mut start = 0;
    if text[..first].chars().count() > LINE_CHARS / 3 {
        // Keep about a third of the room for text before the match.
        start = text[..first].char_indices().rev().nth(LINE_CHARS / 3).map_or(0, |(i, _)| i);
    }
    let end = text[start..].char_indices().nth(LINE_CHARS).map_or(text.len(), |(i, _)| start + i);
    let utf16 = |byte: usize| text[start..byte.clamp(start, end)].encode_utf16().count();
    let ranges = found.iter().filter(|(s, _)| *s < end).map(|&(s, e)| (utf16(s), utf16(e))).filter(|(s, e)| e > s).collect();
    let prefix = if start > 0 { "…" } else { "" };
    let shift = prefix.encode_utf16().count();
    LineMatch {
        line,
        text: format!("{prefix}{}", &text[start..end]),
        ranges: if shift == 0 { ranges } else { ranges.into_iter().map(|(s, e)| (s + shift, e + shift)).collect() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_paths() {
        let files: Vec<String> = ["README.md", "docs/guide.md", "src/main.rs", "docs/gallery/index.md", "Cargo.toml"].map(String::from).into();
        let got = quick_open(&files, "guid", 10);
        assert_eq!(got[0].path, "docs/guide.md");
        assert_eq!(got[0].indices, [5, 6, 7, 8]);
        let empty: Vec<_> = quick_open(&files, "", 10).into_iter().map(|m| m.path).collect();
        assert_eq!(empty, ["README.md", "docs/guide.md", "docs/gallery/index.md"]);
        assert!(quick_open(&files, "zzz", 10).is_empty());
    }

    #[test]
    fn search_files() {
        let root = std::env::temp_dir().join(format!("maido-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.md"), "Hello world\nnothing\nhello again, World\n").unwrap();
        std::fs::write(root.join("sub/b.txt"), "é world é\n").unwrap();
        std::fs::write(root.join("bin.dat"), b"world\0\x01").unwrap();
        std::fs::write(root.join(".gitignore"), "ignored.md\n").unwrap();
        std::fs::write(root.join("ignored.md"), "world\n").unwrap();
        let generation = Arc::new(AtomicU64::new(1));
        let mut events = Vec::new();
        search(&[(String::new(), root.clone())], "world", &generation, 1, |e| events.push(e));
        let mut found: Vec<(String, Vec<(u64, Vec<(usize, usize)>)>)> = events
            .iter()
            .filter_map(|e| match e {
                SearchEvent::File { path, matches } => Some((path.clone(), matches.iter().map(|m| (m.line, m.ranges.clone())).collect())),
                _ => None,
            })
            .collect();
        found.sort();
        assert_eq!(found, [("a.md".to_string(), vec![(1, vec![(6, 11)]), (3, vec![(13, 18)])]), ("sub/b.txt".to_string(), vec![(1, vec![(2, 7)])])]);
        assert!(matches!(events.last(), Some(SearchEvent::Done { files: 2, matches: 3, truncated: false })));
        // Capitals make it case-sensitive.
        let mut events = Vec::new();
        search(&[(String::new(), root.clone())], "World", &generation, 1, |e| events.push(e));
        assert!(matches!(events.last(), Some(SearchEvent::Done { matches: 1, .. })));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn long_lines_clipped() {
        let text = format!("{}needle{}", "x".repeat(500), "y".repeat(500));
        let m = clip(1, &text, &[(500, 506)]);
        assert!(m.text.starts_with('…'));
        let (s, e) = m.ranges[0];
        let units: Vec<u16> = m.text.encode_utf16().collect();
        assert_eq!(String::from_utf16(&units[s..e]).unwrap(), "needle");
        assert!(m.text.chars().count() <= LINE_CHARS + 1);
    }
}
