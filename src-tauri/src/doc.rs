//! Markdown documents as top-level blocks (C1): each block has its source line
//! range, a key that changes when its HTML would, and its sanitized HTML.
//! A change re-parses from the block before the edit onward (H5) and reuses
//! the HTML of every block whose key is unchanged.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options};
use serde::Serialize;

/// A heading, for the outline and `#anchor` links.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Heading {
    pub level: u8,
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// Hash of what the block renders from; the frontend's key is this plus
    /// an occurrence count (identical blocks repeat).
    pub hash: String,
    /// Hash of the block's source lines alone.
    source_hash: String,
    /// First and last source line, 1-based.
    pub line: usize,
    pub end: usize,
    pub html: Arc<str>,
    pub heading: Option<Heading>,
    /// Code blocks with a language, in document order, for highlighting (H4).
    pub code: Vec<(String, String)>,
    pub remote_images: usize,
    /// Has `$math$`, `$$math$$` or a `math` fence: the frontend loads KaTeX (C3).
    pub math: bool,
}

/// One document as Rust keeps it between changes.
pub struct Document {
    source: String,
    has_definitions: bool,
    pub blocks: Vec<Block>,
}

impl Document {
    /// Parse `source` (the file at `path`; relative images resolve from its folder).
    pub fn parse(source: String, path: &Path, root: &Path) -> Document {
        let ctx = Context { dir: path.parent().unwrap_or(root).to_path_buf(), root: root.to_path_buf() };
        let has_definitions = has_definitions(&source);
        let raw = parse_blocks(&source, 1, true, &ctx, &HashMap::new());
        let blocks = finish(raw, has_definitions);
        Document { source, has_definitions, blocks }
    }

    /// Replace the source; re-parse from the block before the first changed
    /// line onward. Returns false if nothing changed.
    pub fn update(&mut self, source: String, path: &Path, root: &Path) -> bool {
        if source == self.source {
            return false;
        }
        let ctx = Context { dir: path.parent().unwrap_or(root).to_path_buf(), root: root.to_path_buf() };
        let has_definitions = has_definitions(&source);
        let cache: HashMap<&str, &Block> = self.blocks.iter().map(|b| (b.source_hash.as_str(), b)).collect();
        // A `---` anywhere can close front matter opened on line 1.
        let front_matter = source.starts_with("---") || self.source.starts_with("---");
        let restart = if has_definitions || self.has_definitions || front_matter { None } else { restart_block(&self.source, &source, &self.blocks) };
        let raw = match restart {
            Some(r) => {
                let line = self.blocks[r].line;
                let offset = line_offset(&source, line);
                let mut raw: Vec<RawBlock> = self.blocks[..r].iter().map(RawBlock::from_block).collect();
                raw.extend(parse_blocks(&source[offset..], line, false, &ctx, &cache));
                raw
            }
            None => parse_blocks(&source, 1, true, &ctx, &cache),
        };
        self.blocks = finish(raw, has_definitions);
        self.source = source;
        self.has_definitions = has_definitions;
        true
    }
}

struct Context {
    dir: PathBuf,
    root: PathBuf,
}

/// A block before heading ids are assigned (they depend on the whole document).
struct RawBlock {
    source_hash: String,
    line: usize,
    end: usize,
    html: Arc<str>,
    heading: Option<(u8, String)>,
    code: Vec<(String, String)>,
    remote_images: usize,
    math: bool,
}

impl RawBlock {
    fn from_block(b: &Block) -> RawBlock {
        RawBlock {
            source_hash: b.source_hash.clone(),
            line: b.line,
            end: b.end,
            html: strip_heading_id(&b.html, b.heading.as_ref()),
            heading: b.heading.as_ref().map(|h| (h.level, h.text.clone())),
            code: b.code.clone(),
            remote_images: b.remote_images,
            math: b.math,
        }
    }
}

/// Link reference and footnote definitions change blocks anywhere in the
/// document, so documents that have them (or look like they do) are always
/// parsed whole. Over-matching only costs a full parse.
fn has_definitions(source: &str) -> bool {
    source.lines().any(|l| {
        let t = l.trim_start_matches(' ');
        l.len() - t.len() <= 3 && t.starts_with('[') && t.contains("]:")
    })
}

fn line_offset(source: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    source.match_indices('\n').nth(line - 2).map_or(source.len(), |(i, _)| i + 1)
}

/// The old block to re-parse from: the one before the block holding the first
/// changed line, moved back until the line above it is blank (so no open
/// paragraph or list can reach into it). None means parse the whole document.
fn restart_block(old: &str, new: &str, blocks: &[Block]) -> Option<usize> {
    let common = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let changed_line = old[..common].matches('\n').count() + 1;
    let containing = blocks.iter().rposition(|b| b.line <= changed_line)?;
    let old_lines: Vec<&str> = old.split('\n').collect();
    let mut r = containing.checked_sub(1)?;
    while r > 0 {
        let above = blocks[r].line - 1;
        if old_lines.get(above - 1).is_some_and(|l| l.trim().is_empty()) {
            return Some(r);
        }
        r -= 1;
    }
    None
}

fn options(front_matter: bool) -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.alerts = true;
    o.extension.math_dollars = true;
    o.extension.math_code = true;
    if front_matter {
        o.extension.front_matter_delimiter = Some("---".into());
    }
    o.render.tasklist_classes = true;
    o
}

/// Parse `source` (which starts at document line `first_line`) into blocks.
/// Blocks whose hash is in `cache` reuse its HTML instead of rendering again.
fn parse_blocks(source: &str, first_line: usize, front_matter: bool, ctx: &Context, cache: &HashMap<&str, &Block>) -> Vec<RawBlock> {
    let arena = Arena::new();
    let mut opts = options(front_matter);
    let remote = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    {
        let dir = ctx.dir.clone();
        let root = ctx.root.clone();
        let remote = remote.clone();
        opts.extension.image_url_rewriter = Some(Arc::new(move |url: &str| rewrite_image(url, &dir, &root, &remote)));
    }
    let doc = comrak::parse_document(&arena, source, &opts);
    let lines: Vec<&str> = source.split('\n').collect();
    let offset = first_line - 1;

    // Footnote definitions end up after the last block; they render as one
    // list, so they are one block.
    let mut footnotes = Vec::new();
    let mut out = Vec::new();
    for node in doc.children() {
        if matches!(node.data().value, NodeValue::FootnoteDefinition(_)) {
            footnotes.push(node);
            continue;
        }
        let pos = node.data().sourcepos;
        let (start, end) = (pos.start.line, pos.end.line.max(pos.start.line));
        // With its final newline: a code fence left open renders differently
        // at the end of a file with and without one.
        let mut text = lines.get(start - 1..end.min(lines.len())).map(|l| l.join("\n")).unwrap_or_default();
        if end < lines.len() {
            text.push('\n');
        }
        let source_hash = hash(&text);
        let heading = match &node.data().value {
            NodeValue::Heading(h) => Some((h.level, plain_text(node))),
            _ => None,
        };
        let code = code_blocks(node);
        let math = has_math(node);
        let (html, remote_images) = match cache.get(source_hash.as_str()) {
            Some(b) => (strip_heading_id(&b.html, b.heading.as_ref()), b.remote_images),
            None => {
                remote.store(0, std::sync::atomic::Ordering::Relaxed);
                let html = render(node, &opts);
                (html, remote.load(std::sync::atomic::Ordering::Relaxed))
            }
        };
        out.push(RawBlock { source_hash, line: start + offset, end: end + offset, html, heading, code, remote_images, math });
    }
    if !footnotes.is_empty() {
        let group = arena.alloc(comrak::nodes::Ast::new(NodeValue::Document, (1, 1).into()).into());
        let first = footnotes[0].data().sourcepos.start.line;
        let last = footnotes.iter().map(|n| n.data().sourcepos.end.line).max().unwrap_or(first);
        let mut code = Vec::new();
        let mut math = false;
        for n in &footnotes {
            code.extend(code_blocks(n));
            math |= has_math(n);
            group.append(n);
        }
        remote.store(0, std::sync::atomic::Ordering::Relaxed);
        let html = render(group, &opts);
        let remote_images = remote.load(std::sync::atomic::Ordering::Relaxed);
        out.push(RawBlock { source_hash: hash(&html), line: first + offset, end: last + offset, html, heading: None, code, remote_images, math });
    }
    out
}

/// Assign heading ids (unique across the document) and final hashes.
fn finish(raw: Vec<RawBlock>, has_definitions: bool) -> Vec<Block> {
    let mut slugs = HashSet::new();
    raw.into_iter()
        .map(|b| {
            let heading = b.heading.map(|(level, text)| Heading { level, id: unique_slug(&text, &mut slugs), text });
            let html: Arc<str> = match &heading {
                Some(h) => add_heading_id(&b.html, h).into(),
                None => b.html,
            };
            // With definitions in play a block's HTML depends on the rest of
            // the document (footnote numbers, link targets): hash the HTML.
            let hash = match (&heading, has_definitions) {
                (_, true) => hash(&html),
                (Some(h), false) => hash(&format!("{}\0{}", b.source_hash, h.id)),
                (None, false) => b.source_hash.clone(),
            };
            Block { hash, source_hash: b.source_hash, line: b.line, end: b.end, html, heading, code: b.code, remote_images: b.remote_images, math: b.math }
        })
        .collect()
}

fn hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
}

fn render<'a>(node: &'a AstNode<'a>, opts: &Options) -> Arc<str> {
    if let NodeValue::FrontMatter(text) = &node.data().value {
        let body = text.trim().trim_start_matches("---").trim_end_matches("---").trim_matches('\n');
        return format!("<details class=\"front-matter\"><summary>Front matter</summary><pre>{}</pre></details>", escape(body)).into();
    }
    let mut html = String::new();
    let _ = comrak::format_html(node, opts, &mut html);
    SANITIZER.clean(&html).to_string().into()
}

fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    for d in node.descendants() {
        match &d.data().value {
            NodeValue::Text(t) => text.push_str(t),
            NodeValue::Code(c) => text.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => text.push(' '),
            _ => {}
        }
    }
    text
}

fn code_blocks<'a>(node: &'a AstNode<'a>) -> Vec<(String, String)> {
    node.descendants()
        .filter_map(|d| match &d.data().value {
            NodeValue::CodeBlock(c) => {
                let lang = c.info.split_whitespace().next().unwrap_or("");
                (!lang.is_empty()).then(|| (lang.to_string(), c.literal.clone()))
            }
            _ => None,
        })
        .collect()
}

fn has_math<'a>(node: &'a AstNode<'a>) -> bool {
    node.descendants().any(|d| match &d.data().value {
        NodeValue::Math(_) => true,
        NodeValue::CodeBlock(c) => c.info.split_whitespace().next() == Some("math"),
        _ => false,
    })
}

/// GitHub-style heading slugs: lowercase, spaces to `-`, punctuation dropped,
/// repeats numbered (`intro`, `intro-1`).
fn unique_slug(text: &str, used: &mut HashSet<String>) -> String {
    let base: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect();
    let mut slug = base.clone();
    let mut n = 1;
    while !used.insert(slug.clone()) {
        slug = format!("{base}-{n}");
        n += 1;
    }
    slug
}

fn add_heading_id(html: &str, h: &Heading) -> String {
    let tag = format!("<h{}", h.level);
    match html.strip_prefix(&tag) {
        Some(rest) => format!("{tag} id=\"{}\"{rest}", escape(&h.id)),
        None => html.to_string(),
    }
}

fn strip_heading_id(html: &str, h: Option<&Heading>) -> Arc<str> {
    match h {
        Some(h) => html.replacen(&format!(" id=\"{}\"", escape(&h.id)), "", 1).into(),
        None => html.into(),
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Images: workspace files become `maido-local:<absolute path>` (the frontend
/// maps them to the asset protocol), remote ones `maido-remote:<url>` (shown
/// only once allowed, F4); anything else, or a path outside the workspace,
/// is blocked.
fn rewrite_image(url: &str, dir: &Path, root: &Path, remote: &std::sync::atomic::AtomicUsize) -> String {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        remote.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return format!("maido-remote:{url}");
    }
    if lower.starts_with("data:image/") && !lower.starts_with("data:image/svg") {
        return url.to_string();
    }
    if url.contains(':') && !url.starts_with('/') && !url.starts_with('.') {
        return "maido-blocked:".into();
    }
    let path = url.split(['#', '?']).next().unwrap_or("");
    let path = percent_decode(path);
    let full = if path.starts_with('/') { PathBuf::from(&path) } else { normalize(&dir.join(&path)) };
    if !full.starts_with(root) {
        return "maido-blocked:".into();
    }
    format!("maido-local:{}", percent_encode(&full.to_string_lossy()))
}

/// Resolve `.` and `..` without touching the file system.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// What document HTML may contain (F3): ammonia's defaults plus what comrak
/// emits for tables, task lists, footnotes, alerts and math, and the image schemes
/// above. Raw HTML in the document is already dropped by comrak.
static SANITIZER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut b = ammonia::Builder::default();
    b.add_tags(["input", "section", "details", "summary"])
        .add_tag_attributes("input", ["type", "checked", "disabled"])
        .add_tag_attributes("th", ["align"])
        .add_tag_attributes("td", ["align"])
        .add_tag_attributes("code", ["class", "data-math-style"])
        .add_tag_attributes("span", ["data-math-style"])
        .add_tag_attributes("li", ["class", "id"])
        .add_tag_attributes("ul", ["class"])
        .add_tag_attributes("section", ["class", "data-footnotes"])
        .add_tag_attributes("sup", ["class"])
        .add_tag_attributes("div", ["class"])
        .add_tag_attributes("p", ["class"])
        .add_tag_attributes("a", ["id", "class", "data-footnote-ref", "data-footnote-backref", "data-footnote-backref-idx", "aria-label"])
        .add_url_schemes(["maido-local", "maido-remote", "maido-blocked"])
        .link_rel(None)
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("input", "type") => (value == "checkbox").then_some(value.into()),
            (_, "class") => {
                let ok = value.split(' ').all(|c| {
                    c.starts_with("language-")
                        || c.starts_with("markdown-alert")
                        || matches!(c, "task-list-item" | "contains-task-list" | "footnotes" | "footnote-ref" | "footnote-backref")
                });
                ok.then_some(value.into())
            }
            (_, "data-math-style") => matches!(value, "inline" | "display").then_some(value.into()),
            (_, "id") => (value.starts_with("fn-") || value.starts_with("fnref-")).then_some(value.into()),
            _ => Some(value.into()),
        });
    b
});

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> Document {
        Document::parse(src.into(), Path::new("/w/a.md"), Path::new("/w"))
    }

    #[test]
    fn blocks_and_lines() {
        let d = doc("# Title\n\nSome *text*.\n\n```rust\nfn main() {}\n```\n");
        assert_eq!(d.blocks.len(), 3);
        assert_eq!((d.blocks[0].line, d.blocks[0].end), (1, 1));
        assert_eq!(&*d.blocks[0].html, "<h1 id=\"title\">Title</h1>\n");
        assert_eq!((d.blocks[2].line, d.blocks[2].end), (5, 7));
        assert_eq!(d.blocks[2].code, vec![("rust".into(), "fn main() {}\n".into())]);
        assert!(d.blocks[2].html.contains("class=\"language-rust\""));
    }

    #[test]
    fn heading_ids_are_unique() {
        let d = doc("# Intro\n\n## Intro\n\n## C++ & you!\n");
        let ids: Vec<_> = d.blocks.iter().map(|b| b.heading.as_ref().unwrap().id.as_str()).collect();
        assert_eq!(ids, ["intro", "intro-1", "c--you"]);
    }

    #[test]
    fn raw_html_and_scripts_dropped() {
        let d = doc("<script>alert(1)</script>\n\n[x](javascript:alert(1))\n\n<img src=x onerror=alert(1)>\n");
        let html: String = d.blocks.iter().map(|b| &*b.html).collect();
        assert!(!html.contains("script") && !html.contains("javascript") && !html.contains("onerror"), "{html}");
    }

    #[test]
    fn images_rewritten() {
        let d = doc("![a](img/x%20y.png) ![b](https://e.com/b.png) ![c](../../etc/passwd) ![d](file:///etc/passwd)\n");
        let html = &*d.blocks[0].html;
        assert!(html.contains("src=\"maido-local:/w/img/x%20y.png\""), "{html}");
        assert!(html.contains("src=\"maido-remote:https://e.com/b.png\""), "{html}");
        assert!(html.contains("src=\"maido-blocked:\" alt=\"c\"") && !html.contains("passwd"), "{html}");
        assert_eq!(d.blocks[0].remote_images, 1);
    }

    #[test]
    fn front_matter_tasks_footnotes() {
        let d = doc("---\ntitle: x\n---\n\n- [x] done\n- [ ] todo\n\nA[^1] and B[^2].\n\n[^1]: one\n[^2]: two\n");
        assert!(d.blocks[0].html.starts_with("<details class=\"front-matter\">"));
        assert!(d.blocks[1].html.contains("type=\"checkbox\""), "{}", d.blocks[1].html);
        let last = d.blocks.last().unwrap();
        assert!(last.html.contains("<section class=\"footnotes\""), "{}", last.html);
        assert_eq!(last.html.matches("<ol>").count(), 1, "{}", last.html);
        assert!(d.blocks[2].html.contains("href=\"#fn-1\""), "{}", d.blocks[2].html);
    }

    #[test]
    fn unchanged_blocks_keep_hashes() {
        let mut d = doc("# A\n\none\n\ntwo\n\nthree\n");
        let before: Vec<_> = d.blocks.iter().map(|b| b.hash.clone()).collect();
        d.update("# A\n\none\n\nTWO\n\nthree\n".into(), Path::new("/w/a.md"), Path::new("/w"));
        let after: Vec<_> = d.blocks.iter().map(|b| b.hash.clone()).collect();
        assert_eq!(before[0], after[0]);
        assert_eq!(before[1], after[1]);
        assert_ne!(before[2], after[2]);
        assert_eq!(before[3], after[3]);
    }

    #[test]
    fn math_marked() {
        let d = doc("Inline $a^2$ here.\n\n$$\nx = 1\n$$\n\n```math\ny\n```\n\nPlain $5 and $6.\n");
        let math: Vec<bool> = d.blocks.iter().map(|b| b.math).collect();
        assert_eq!(math, [true, true, true, false]);
        assert!(d.blocks[0].html.contains("<span data-math-style=\"inline\">a^2</span>"), "{}", d.blocks[0].html);
        assert!(d.blocks[1].html.contains("data-math-style=\"display\""), "{}", d.blocks[1].html);
        assert!(d.blocks[2].html.contains("<code class=\"language-math\" data-math-style=\"display\">"), "{}", d.blocks[2].html);
    }

    /// H5: the incremental path must give exactly what a full parse gives.
    #[test]
    fn incremental_matches_full_parse() {
        const PIECES: &[&str] = &[
            "# Heading", "## Heading", "para line", "another line", "", "", "- item", "  - nested", "1. one", "> quote",
            "```", "```rust", "fn x() {}", "~~~", "---", "===", "| a | b |", "|---|---|", "| 1 | 2 |", "    indented",
            "<div>", "</div>", "- [ ] task", "***", "text *em*", "  continued", "* star item", "", "$$", "a $x$ b", "c$ d", "x^2 $$",
            "```math",
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut rand = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let (path, root) = (Path::new("/w/a.md"), Path::new("/w"));
        for _ in 0..400 {
            let mut lines: Vec<String> = (0..rand(40) + 1).map(|_| PIECES[rand(PIECES.len())].to_string()).collect();
            let mut d = Document::parse(lines.join("\n"), path, root);
            for _ in 0..8 {
                let at = rand(lines.len() + 1);
                match rand(3) {
                    0 => lines.insert(at, PIECES[rand(PIECES.len())].into()),
                    1 if at < lines.len() => {
                        lines.remove(at);
                    }
                    _ if at < lines.len() => lines[at] = PIECES[rand(PIECES.len())].into(),
                    _ => lines.push(String::new()),
                }
                let src = lines.join("\n");
                d.update(src.clone(), path, root);
                let full = Document::parse(src.clone(), path, root);
                let brief = |b: &[Block]| b.iter().map(|b| format!("{}-{} {}", b.line, b.end, b.html.replace('\n', " "))).collect::<Vec<_>>();
                assert_eq!(brief(&d.blocks), brief(&full.blocks), "source:\n{src}");
                assert_eq!(d.blocks, full.blocks, "source:\n{src}");
            }
        }
    }
}
