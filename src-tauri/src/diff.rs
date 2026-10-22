//! The diff window (E5): a line diff of two texts with the changed words
//! marked inside changed lines, and a block-by-block diff of two Markdown
//! documents as rendered. Read-only.

use std::time::Duration;

use serde::Serialize;
use similar::{Algorithm, ChangeTag, DiffTag, TextDiff, capture_diff_slices};

use crate::doc::Block;

/// Giving up on an optimal diff after this; the result is still correct,
/// just less minimal.
const TIMEOUT: Duration = Duration::from_secs(2);

/// A run of text inside a line; `changed` marks the words that differ.
#[derive(Debug, PartialEq, Serialize)]
pub struct Segment {
    pub text: String,
    pub changed: bool,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Side {
    /// 1-based line number.
    pub line: usize,
    pub segments: Vec<Segment>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Equal,
    Delete,
    Insert,
    Replace,
}

/// One row of the side-by-side view; the inline view reads the same rows.
#[derive(Debug, PartialEq, Serialize)]
pub struct Row {
    pub kind: Kind,
    pub left: Option<Side>,
    pub right: Option<Side>,
}

fn plain(line: usize, text: &str) -> Side {
    Side { line, segments: vec![Segment { text: text.trim_end_matches(['\n', '\r']).to_string(), changed: false }] }
}

/// Word-level segments for a changed pair of lines.
fn words(old: &str, new: &str) -> (Vec<Segment>, Vec<Segment>) {
    let old = old.trim_end_matches(['\n', '\r']);
    let new = new.trim_end_matches(['\n', '\r']);
    let diff = TextDiff::configure().timeout(TIMEOUT).diff_words(old, new);
    let (mut left, mut right) = (Vec::<Segment>::new(), Vec::<Segment>::new());
    let push = |side: &mut Vec<Segment>, text: &str, changed: bool| match side.last_mut() {
        Some(last) if last.changed == changed => last.text.push_str(text),
        _ => side.push(Segment { text: text.to_string(), changed }),
    };
    for change in diff.iter_all_changes() {
        let text = change.value();
        match change.tag() {
            ChangeTag::Equal => {
                push(&mut left, text, false);
                push(&mut right, text, false);
            }
            ChangeTag::Delete => push(&mut left, text, true),
            ChangeTag::Insert => push(&mut right, text, true),
        }
    }
    (left, right)
}

/// Line diff of `old` against `new`.
pub fn lines(old: &str, new: &str) -> Vec<Row> {
    let diff = TextDiff::configure().algorithm(Algorithm::Patience).timeout(TIMEOUT).diff_lines(old, new);
    let (a, b): (Vec<&str>, Vec<&str>) = (diff.iter_old_slices().collect(), diff.iter_new_slices().collect());
    let mut rows = Vec::new();
    for op in diff.ops() {
        let (tag, o, n) = op.as_tag_tuple();
        match tag {
            DiffTag::Equal => {
                for (i, j) in o.zip(n) {
                    rows.push(Row { kind: Kind::Equal, left: Some(plain(i + 1, a[i])), right: Some(plain(j + 1, b[j])) });
                }
            }
            DiffTag::Delete => rows.extend(o.map(|i| Row { kind: Kind::Delete, left: Some(plain(i + 1, a[i])), right: None })),
            DiffTag::Insert => rows.extend(n.map(|j| Row { kind: Kind::Insert, left: None, right: Some(plain(j + 1, b[j])) })),
            DiffTag::Replace => {
                // Pair lines up in order; the longer side's extra lines stand alone.
                let (o, n): (Vec<usize>, Vec<usize>) = (o.collect(), n.collect());
                for k in 0..o.len().max(n.len()) {
                    match (o.get(k), n.get(k)) {
                        (Some(&i), Some(&j)) => {
                            let (l, r) = words(a[i], b[j]);
                            rows.push(Row { kind: Kind::Replace, left: Some(Side { line: i + 1, segments: l }), right: Some(Side { line: j + 1, segments: r }) });
                        }
                        (Some(&i), None) => rows.push(Row { kind: Kind::Delete, left: Some(plain(i + 1, a[i])), right: None }),
                        (None, Some(&j)) => rows.push(Row { kind: Kind::Insert, left: None, right: Some(plain(j + 1, b[j])) }),
                        (None, None) => {}
                    }
                }
            }
        }
    }
    rows
}

/// One row of the rendered comparison: a block on either side or both.
#[derive(Serialize)]
pub struct BlockRow {
    pub kind: Kind,
    pub left: Option<String>,
    pub right: Option<String>,
}

/// Rendered Markdown compared block by block, by the block hashes (C1).
pub fn blocks(old: &[Block], new: &[Block]) -> Vec<BlockRow> {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.iter().map(|x| x.hash.as_str()).collect(), new.iter().map(|x| x.hash.as_str()).collect());
    let html = |x: &Block| Some(x.html.to_string());
    let mut rows = Vec::new();
    for op in capture_diff_slices(Algorithm::Patience, &a, &b) {
        let (tag, o, n) = op.as_tag_tuple();
        match tag {
            DiffTag::Equal => rows.extend(o.map(|i| BlockRow { kind: Kind::Equal, left: html(&old[i]), right: html(&old[i]) })),
            DiffTag::Delete => rows.extend(o.map(|i| BlockRow { kind: Kind::Delete, left: html(&old[i]), right: None })),
            DiffTag::Insert => rows.extend(n.map(|j| BlockRow { kind: Kind::Insert, left: None, right: html(&new[j]) })),
            DiffTag::Replace => {
                let (o, n): (Vec<usize>, Vec<usize>) = (o.collect(), n.collect());
                for k in 0..o.len().max(n.len()) {
                    let (l, r) = (o.get(k).map(|&i| &old[i]), n.get(k).map(|&j| &new[j]));
                    let kind = match (l, r) {
                        (Some(_), Some(_)) => Kind::Replace,
                        (Some(_), None) => Kind::Delete,
                        _ => Kind::Insert,
                    };
                    rows.push(BlockRow { kind, left: l.and_then(html), right: r.and_then(html) });
                }
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(segments: &[Segment]) -> String {
        segments.iter().map(|s| if s.changed { format!("[{}]", s.text) } else { s.text.clone() }).collect()
    }

    #[test]
    fn line_and_word_changes() {
        let rows = lines("a\nthe quick fox\nc\nd\n", "a\nthe slow fox\nc\nnew\nd\n");
        let kinds: Vec<_> = rows.iter().map(|r| &r.kind).collect();
        assert_eq!(kinds, [&Kind::Equal, &Kind::Replace, &Kind::Equal, &Kind::Insert, &Kind::Equal]);
        let r = &rows[1];
        assert_eq!(text(&r.left.as_ref().unwrap().segments), "the [quick] fox");
        assert_eq!(text(&r.right.as_ref().unwrap().segments), "the [slow] fox");
        assert_eq!(rows[3].right.as_ref().unwrap().line, 4);
        assert_eq!(rows[4].left.as_ref().unwrap().line, 4);
        assert_eq!(rows[4].right.as_ref().unwrap().line, 5);
    }

    #[test]
    fn identical_and_empty() {
        assert!(lines("x\n", "x\n").iter().all(|r| r.kind == Kind::Equal));
        assert_eq!(lines("", "a\n").len(), 1);
        assert_eq!(lines("a\nb\n", "").iter().filter(|r| r.kind == Kind::Delete).count(), 2);
    }
}
