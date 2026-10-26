//! LaTeX files (`.tex`, `.latex`) read as documents: the source is turned into
//! Markdown with math, and then goes through the same blocks, sanitizing and
//! KaTeX as any Markdown file (so incremental re-parse, find, outline, print
//! and export all work unchanged).
//!
//! This is a reader's translation, not a TeX engine: sections, emphasis,
//! lists, quotes, verbatim code, simple tables, figures (pictures only),
//! links, citations (shown as `[key]`) and all math. Macros you define
//! yourself, packages and page layout are not run; unknown commands lose
//! their name and keep their text. Lines stay in step with the source where
//! they can (so scroll sync works); a few constructs add or drop a line.

/// `src` as Markdown.
pub fn to_markdown(src: &str) -> String {
    let raw: Vec<&str> = src.lines().collect();
    let plain: Vec<&str> = raw.iter().map(|l| strip_comment(l)).collect();
    let uncommented = plain.join("\n");
    let has_document = uncommented.contains("\\begin{document}");
    let mut conv = Conv {
        title: find_group(&uncommented, "\\title"),
        author: find_group(&uncommented, "\\author"),
        date: find_group(&uncommented, "\\date"),
        ..Conv::default()
    };

    let mut out: Vec<String> = Vec::with_capacity(raw.len() + 8);
    let mut body = !has_document;
    let mut ended = false;
    let mut verbatim: Option<(String, bool)> = None; // (environment, dropped)
    let mut display: Option<Display> = None;

    for (n, line) in plain.iter().enumerate() {
        let t = line.trim();

        if let Some((name, dropped)) = &verbatim {
            if raw[n].contains(&format!("\\end{{{name}}}")) {
                out.push(if *dropped { String::new() } else { "```".into() });
                verbatim = None;
            } else {
                out.push(if *dropped { String::new() } else { raw[n].to_string() });
            }
            continue;
        }
        if !body {
            if t.contains("\\begin{document}") {
                body = true;
            }
            out.push(String::new());
            continue;
        }
        if ended || t.starts_with("\\end{document}") {
            ended = true;
            out.push(String::new());
            continue;
        }

        if let Some(d) = &mut display {
            match d.closer.find(line) {
                Some(at) => {
                    let before = clean_math(&line[..at]);
                    if !before.trim().is_empty() {
                        d.push(&mut out, before);
                    }
                    if let (false, Some((_, end))) = (d.first, d.wrap) {
                        if let Some(last) = out.last_mut() {
                            last.push_str(end);
                        }
                    }
                    out.push("```".into());
                    display = None;
                }
                None => d.push(&mut out, clean_math(line)),
            }
            continue;
        }

        let next_blank = plain.get(n + 1).is_none_or(|l| l.trim().is_empty());

        if let Some((closer, wrap, rest)) = display_open(t) {
            match closer.find(rest) {
                Some(at) => {
                    let (pre, post) = wrap.unwrap_or(("", ""));
                    let after = conv.text(&rest[at + closer.token().len()..], next_blank);
                    out.push(format!("$${pre}{}{post}$${after}", clean_math(&rest[..at]).trim()));
                }
                None => {
                    out.push("```math".into());
                    let mut d = Display { closer, wrap, first: true };
                    if !rest.trim().is_empty() {
                        d.push(&mut out, clean_math(rest));
                    }
                    display = Some(d);
                }
            }
            continue;
        }

        if let Some((name, rest)) = begin_of(t) {
            let base = name.trim_end_matches('*');
            match base {
                "verbatim" | "Verbatim" | "lstlisting" | "minted" | "alltt" => {
                    out.push(format!("```{}", code_language(base, rest)));
                    verbatim = Some((name.to_string(), false));
                }
                "comment" => {
                    out.push(String::new());
                    verbatim = Some((name.to_string(), true));
                }
                "itemize" | "enumerate" | "description" => {
                    if !conv.lists.is_empty() {
                        // A nested list sits straight under its item: a blank line would loosen the list.
                    } else {
                        out.push(String::new());
                    }
                    conv.lists.push(match base {
                        "enumerate" => List::Ordered,
                        "description" => List::Description,
                        _ => List::Bullets,
                    });
                }
                "quote" | "quotation" => {
                    out.push(String::new());
                    conv.quote += 1;
                }
                "tabular" | "tabularx" | "longtable" | "array" => {
                    out.push(String::new());
                    conv.table = Some(0);
                }
                "abstract" => out.push("**Abstract**".into()),
                "thebibliography" => out.push("**References**".into()),
                "proof" => out.push("*Proof.*".into()),
                "theorem" | "lemma" | "proposition" | "corollary" | "definition" | "remark" | "example" | "claim" | "conjecture" | "note" => {
                    let note = optional(rest.trim_start()).0.map(|n| conv.text(n, true)).filter(|n| !n.is_empty());
                    let mut label = base[..1].to_uppercase() + &base[1..];
                    if let Some(note) = note {
                        label = format!("{label} ({note})");
                    }
                    out.push(format!("**{label}.**"));
                }
                _ => out.push(String::new()),
            }
            continue;
        }
        if let Some(name) = end_of(t) {
            match name.trim_end_matches('*') {
                "itemize" | "enumerate" | "description" => {
                    conv.lists.pop();
                    if conv.lists.is_empty() {
                        out.push(String::new());
                    }
                }
                "quote" | "quotation" => {
                    conv.quote = conv.quote.saturating_sub(1);
                    out.push(String::new());
                }
                "tabular" | "tabularx" | "longtable" | "array" => {
                    conv.table = None;
                    out.push(String::new());
                }
                _ => out.push(String::new()),
            }
            continue;
        }

        if t.is_empty() {
            out.push(if conv.quote > 0 { ">".repeat(conv.quote) } else { String::new() });
            continue;
        }

        if let Some(rows) = conv.table {
            if let Some(row) = conv.table_row(t) {
                out.push(row);
                if rows == 0 {
                    let cols = out.last().map_or(1, |r| r.matches(" | ").count() + 1);
                    out.push(format!("|{}", " --- |".repeat(cols)));
                }
                conv.table = Some(rows + 1);
            } else {
                out.push(String::new());
            }
            continue;
        }

        if let Some((level, title, rest)) = heading(t) {
            let title = conv.text(title, true);
            out.push(format!("{} {title}", "#".repeat(level)));
            let rest = conv.text(rest, next_blank);
            if !rest.trim().is_empty() {
                out.push(rest.trim().to_string());
            }
            continue;
        }
        if t.starts_with("\\maketitle") {
            let title = conv.title.clone().map(|s| conv.text(&s, true)).unwrap_or_default();
            out.push(format!("# {title}"));
            let author = conv.author.clone().map(|s| conv.text(&s, true)).unwrap_or_default();
            let date = conv.date.clone().map(|s| conv.text(&s, true)).unwrap_or_default();
            let byline = [author, date].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            if !byline.is_empty() {
                out.push(String::new());
                out.push(format!("*{byline}*"));
            }
            continue;
        }

        // One logical line of prose, an item, or a bibliography entry.
        let (marker, text) = if let Some(rest) = t.strip_prefix("\\item") {
            let (term, rest) = optional(rest.trim_start());
            let indent = conv.lists.len().saturating_sub(1);
            let width = |l: &List| if matches!(l, List::Ordered) { 3 } else { 2 };
            let pad = " ".repeat(conv.lists.iter().take(indent).map(width).sum());
            let kind = conv.lists.last().copied().unwrap_or(List::Bullets);
            let mark = if matches!(kind, List::Ordered) { "1. " } else { "- " };
            let term = term.map(|s| format!("**{}** ", conv.text(s, true).trim())).unwrap_or_default();
            (format!("{pad}{mark}{term}"), rest.trim_start())
        } else if let Some(rest) = t.strip_prefix("\\bibitem") {
            let rest = optional(rest.trim_start()).1;
            let rest = group(rest).map_or(rest, |(_, after)| after);
            ("- ".to_string(), rest)
        } else {
            (String::new(), t)
        };
        let mut text = conv.text(text, next_blank);
        if marker.is_empty() {
            text = protect(&text);
            let pad = " ".repeat(conv.lists.iter().map(|l| if matches!(l, List::Ordered) { 3 } else { 2 }).sum());
            text = format!("{pad}{text}");
        }
        let quote = if conv.quote > 0 { format!("{} ", ">".repeat(conv.quote)) } else { String::new() };
        out.push(format!("{quote}{marker}{text}"));
    }

    let mut md = out.join("\n");
    md.push('\n');
    md
}

#[derive(Clone, Copy)]
enum List {
    Bullets,
    Ordered,
    Description,
}

/// What ends a display-math block, and how its lines are wrapped for KaTeX
/// (which has `aligned` and `gathered`, not `align` and `gather`).
struct Display {
    closer: Closer,
    wrap: Option<(&'static str, &'static str)>,
    first: bool,
}

impl Display {
    fn push(&mut self, out: &mut Vec<String>, line: String) {
        let line = match (self.first, self.wrap) {
            (true, Some((start, _))) => format!("{start}{line}"),
            _ => line,
        };
        self.first = false;
        out.push(line);
    }
}

enum Closer {
    Bracket,
    Dollars,
    End(String),
}

impl Closer {
    fn token(&self) -> String {
        match self {
            Closer::Bracket => "\\]".into(),
            Closer::Dollars => "$$".into(),
            Closer::End(name) => format!("\\end{{{name}}}"),
        }
    }
    fn find(&self, s: &str) -> Option<usize> {
        s.find(&self.token())
    }
}

/// The start of a display-math block on this line: what ends it, how to wrap
/// it, and the text after the opener.
fn display_open(t: &str) -> Option<(Closer, Option<(&'static str, &'static str)>, &str)> {
    if let Some(rest) = t.strip_prefix("\\[") {
        return Some((Closer::Bracket, None, rest));
    }
    if let Some(rest) = t.strip_prefix("$$") {
        return Some((Closer::Dollars, None, rest));
    }
    let (name, rest) = begin_of(t)?;
    let wrap = match name.trim_end_matches('*') {
        "equation" | "displaymath" | "math" => None,
        "align" | "alignat" | "flalign" | "eqnarray" | "split" | "aligned" => Some(("\\begin{aligned}", "\\end{aligned}")),
        "gather" | "multline" => Some(("\\begin{gathered}", "\\end{gathered}")),
        _ => return None,
    };
    // `\begin{alignat}{2}` carries a column count.
    let rest = if name.starts_with("alignat") { group(rest).map_or(rest, |(_, after)| after) } else { rest };
    Some((Closer::End(name.to_string()), wrap, rest))
}

/// `\begin{name}…` at the start of `t`: the name and what follows it.
fn begin_of(t: &str) -> Option<(&str, &str)> {
    let (name, rest) = group(t.strip_prefix("\\begin")?)?;
    Some((name, rest))
}

fn end_of(t: &str) -> Option<&str> {
    group(t.strip_prefix("\\end")?).map(|(name, _)| name)
}

/// The language of a code environment: `minted`'s second argument, or
/// `lstlisting`'s `language=` option.
fn code_language(env: &str, rest: &str) -> String {
    let rest = rest.trim_start();
    let lang = match env {
        "minted" => {
            let after = optional(rest).1;
            group(after).map(|(g, _)| g.to_string())
        }
        "lstlisting" => optional(rest).0.and_then(|opts| {
            opts.split(',').find_map(|o| o.trim().strip_prefix("language=").map(|v| v.trim_matches(['{', '}']).to_ascii_lowercase()))
        }),
        _ => None,
    };
    lang.filter(|l| l.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '#'))).unwrap_or_default()
}

/// `\section{Title}rest` and its relatives: the Markdown level, the title, the rest.
fn heading(t: &str) -> Option<(usize, &str, &str)> {
    let after = t.strip_prefix('\\')?;
    let len = after.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(after.len());
    let level = match &after[..len] {
        "part" | "chapter" => 1,
        "section" => 2,
        "subsection" => 3,
        "subsubsection" => 4,
        "paragraph" => 5,
        "subparagraph" => 6,
        _ => return None,
    };
    let rest = after[len..].trim_start_matches('*');
    let rest = optional(rest.trim_start()).1;
    let (title, rest) = group(rest)?;
    Some((level, title, rest))
}

/// The text inside the first `{…}` after `cmd` (with an optional `[…]` between).
fn find_group(src: &str, cmd: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = src[from..].find(cmd) {
        let after = &src[from + at + cmd.len()..];
        // `\title` and not `\titlepage`.
        if !after.starts_with(|c: char| c.is_ascii_alphabetic()) {
            let (g, _) = group(optional(after.trim_start()).1)?;
            return Some(g.to_string());
        }
        from += at + cmd.len();
    }
    None
}

/// The balanced `{…}` at the start of `s` (after spaces) and what follows. A
/// group left open at the end of the line takes the rest of it.
fn group(s: &str) -> Option<(&str, &str)> {
    let t = s.trim_start();
    if !t.starts_with('{') {
        return None;
    }
    let (mut depth, mut escaped) = (0, false);
    for (i, c) in t.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&t[1..i], &t[i + 1..]));
                }
            }
            _ => {}
        }
    }
    Some((&t[1..], ""))
}

/// An optional `[…]` argument at the start of `s`, and what follows.
fn optional(s: &str) -> (Option<&str>, &str) {
    match s.strip_prefix('[').and_then(|r| r.find(']').map(|i| (&r[..i], &r[i + 1..]))) {
        Some((arg, rest)) => (Some(arg), rest),
        None => (None, s),
    }
}

/// The line without its `%` comment (a `\%` is text).
fn strip_comment(line: &str) -> &str {
    let mut backslashes = 0;
    for (i, c) in line.char_indices() {
        if c == '%' && backslashes % 2 == 0 {
            return &line[..i];
        }
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
    }
    line
}

/// Math with what only matters for numbering taken out (`\label`, `\nonumber`).
fn clean_math(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('\\') {
        out.push_str(&rest[..at]);
        let cmd = &rest[at..];
        let skipped = ["\\nonumber", "\\notag"].iter().find(|c| cmd.starts_with(**c) && !cmd[c.len()..].starts_with(|ch: char| ch.is_ascii_alphabetic()));
        if let Some(c) = skipped {
            rest = &cmd[c.len()..];
        } else if let Some((_, after)) = cmd.strip_prefix("\\label").and_then(group) {
            rest = after;
        } else {
            // Any other command, and the character it escapes, are kept as they are.
            let keep = cmd.chars().take(2).map(char::len_utf8).sum::<usize>();
            out.push_str(&cmd[..keep]);
            rest = &cmd[keep..];
        }
    }
    out.push_str(rest);
    out
}

/// A line of prose that Markdown would take for a list, quote, heading or rule.
fn protect(s: &str) -> String {
    let t = s.trim_start();
    let lead = s.len() - t.len();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    let at = if t.starts_with(['-', '+', '>', '#', '=']) {
        lead
    } else if digits > 0 && t[digits..].starts_with(['.', ')']) {
        lead + digits
    } else {
        return s.to_string();
    };
    let mut out = s.to_string();
    out.insert(at, '\\');
    out
}

#[derive(Default)]
struct Conv {
    title: Option<String>,
    author: Option<String>,
    date: Option<String>,
    lists: Vec<List>,
    quote: usize,
    /// Inside a table: the rows written so far.
    table: Option<usize>,
    /// Inside inline math: what closes it (`$` or `\)`).
    math: Option<&'static str>,
}

impl Conv {
    /// One line of prose as Markdown; `last` when the paragraph ends here (a
    /// `\\` there needs no line break).
    fn text(&mut self, s: &str, last: bool) -> String {
        let out = self.inline(s);
        let body = out.trim_end_matches(|c: char| c.is_whitespace() || c == '\u{1}');
        let broke = out[body.len()..].contains('\u{1}');
        let mut text = body.replace('\u{1}', " ");
        if broke && !last {
            text.push('\\');
        }
        text
    }

    fn inline(&mut self, s: &str) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < s.len() {
            let rest = &s[i..];
            let c = rest.chars().next().unwrap_or(' ');

            if let Some(close) = self.math {
                if rest.starts_with("\\$") {
                    out.push_str("\\$");
                    i += 2;
                } else if rest.starts_with(close) {
                    out.push_str("`$");
                    self.math = None;
                    i += close.len();
                } else {
                    if c != '`' {
                        out.push(c);
                    }
                    i += c.len_utf8();
                }
                continue;
            }

            match c {
                '$' if rest.starts_with("$$") => {
                    // Display math inside a line: `$$…$$`.
                    match rest[2..].find("$$") {
                        Some(end) => {
                            out.push_str(&format!("$${}$$", clean_math(&rest[2..2 + end])));
                            i += end + 4;
                        }
                        None => {
                            out.push_str("\\$\\$");
                            i += 2;
                        }
                    }
                }
                '$' => {
                    out.push_str("$`");
                    self.math = Some("$");
                    i += 1;
                }
                '\\' => i += self.command(rest, &mut out),
                '{' | '}' => i += 1,
                '~' => {
                    out.push('\u{a0}');
                    i += 1;
                }
                '-' if rest.starts_with("---") => {
                    out.push('—');
                    i += 3;
                }
                '-' if rest.starts_with("--") => {
                    out.push('–');
                    i += 2;
                }
                '`' if rest.starts_with("``") => {
                    out.push('“');
                    i += 2;
                }
                '`' => {
                    out.push('‘');
                    i += 1;
                }
                '\'' if rest.starts_with("''") => {
                    out.push('”');
                    i += 2;
                }
                '*' | '_' | '[' | ']' | '<' | '>' | '|' => {
                    out.push('\\');
                    out.push(c);
                    i += 1;
                }
                _ => {
                    out.push(c);
                    i += c.len_utf8();
                }
            }
        }
        out
    }

    /// The command at the start of `rest` (which begins with a backslash):
    /// appends its Markdown to `out` and returns the bytes it used.
    fn command(&mut self, rest: &str, out: &mut String) -> usize {
        let after = &rest[1..];
        let Some(next) = after.chars().next() else { return 1 };

        if !next.is_ascii_alphabetic() {
            let used = 1 + next.len_utf8();
            match next {
                '(' => {
                    out.push_str("$`");
                    self.math = Some("\\)");
                }
                '\\' => {
                    out.push('\u{1}');
                    // `\\[2pt]`
                    let tail = &rest[used..];
                    return used + tail.len() - optional(tail).1.len();
                }
                '%' | '&' | '#' | '{' | '}' => out.push(next),
                '$' | '_' => {
                    out.push('\\');
                    out.push(next);
                }
                ',' | ';' | ':' | ' ' | '!' => out.push(' '),
                '\'' | '`' | '^' | '"' | '~' => {
                    let mark = match next {
                        '\'' => '\u{301}',
                        '`' => '\u{300}',
                        '^' => '\u{302}',
                        '"' => '\u{308}',
                        _ => '\u{303}',
                    };
                    let tail = &rest[used..];
                    let (letter, len) = match group(tail) {
                        Some((g, after)) if tail.starts_with('{') => (g.chars().next(), tail.len() - after.len()),
                        _ => (tail.chars().next(), tail.chars().next().map_or(0, char::len_utf8)),
                    };
                    if let Some(l) = letter {
                        out.push(l);
                        out.push(mark);
                    }
                    return used + len;
                }
                _ => {}
            }
            return used;
        }

        let len = after.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(after.len());
        let name = &after[..len];
        let mut tail = &after[len..];
        if tail.starts_with('*') {
            tail = &tail[1..];
        }
        // The space after a command name belongs to the command.
        let spaced = tail.trim_start_matches(' ');
        let consumed = |left: &str| rest.len() - left.len();

        // Text styles.
        let wrap = match name {
            "textbf" | "mathbf" => Some("**"),
            "textit" | "emph" | "textsl" => Some("*"),
            _ => None,
        };
        if let Some(mark) = wrap {
            return match group(spaced) {
                Some((arg, after)) => {
                    let inner = self.inline(arg);
                    let inner = inner.trim();
                    if !inner.is_empty() {
                        out.push_str(&format!("{mark}{inner}{mark}"));
                    }
                    consumed(after)
                }
                None => consumed(tail),
            };
        }
        match name {
            "texttt" | "url" | "path" | "verb" => {
                if name == "verb" {
                    let mut chars = tail.chars();
                    if let Some(d) = chars.next() {
                        if let Some(end) = tail[d.len_utf8()..].find(d) {
                            let code = &tail[d.len_utf8()..d.len_utf8() + end];
                            out.push_str(&format!("`{}`", code.replace('`', "'")));
                            return consumed(&tail[d.len_utf8() * 2 + end..]);
                        }
                    }
                    return consumed(tail);
                }
                let Some((arg, after)) = group(spaced) else { return consumed(tail) };
                if name == "url" {
                    out.push_str(&link(arg, arg));
                } else {
                    out.push_str(&format!("`{}`", arg.replace('`', "'")));
                }
                consumed(after)
            }
            "href" => {
                let Some((target, after)) = group(spaced) else { return consumed(tail) };
                let Some((label, after)) = group(after) else { return consumed(after) };
                let label = self.inline(label);
                out.push_str(&link(target, &label));
                consumed(after)
            }
            "footnote" | "footnotetext" => {
                let Some((arg, after)) = group(spaced) else { return consumed(tail) };
                out.push_str(&format!(" ({})", self.inline(arg).trim()));
                consumed(after)
            }
            "cite" | "citep" | "citet" | "citeauthor" | "citeyear" | "ref" | "autoref" | "cref" | "Cref" | "pageref" | "eqref" => {
                let (_, spaced) = optional(spaced);
                let (_, spaced) = optional(spaced);
                let Some((keys, after)) = group(spaced) else { return consumed(tail) };
                let keys = keys.replace('_', "\\_");
                out.push_str(&if name == "eqref" { format!("({keys})") } else { format!("\\[{keys}\\]") });
                consumed(after)
            }
            "includegraphics" => {
                let (_, spaced) = optional(spaced);
                let Some((path, after)) = group(spaced) else { return consumed(tail) };
                out.push_str(&format!("![]({})", path.trim().replace(' ', "%20").replace(')', "%29").replace('(', "%28")));
                consumed(after)
            }
            "caption" | "captionof" => {
                let (_, spaced) = optional(spaced);
                let Some((arg, after)) = group(spaced) else { return consumed(tail) };
                let text = self.inline(arg);
                out.push_str(&format!("*{}*", text.trim()));
                consumed(after)
            }
            "begin" | "end" => group(spaced).map_or(consumed(tail), |(_, after)| consumed(after)),
            // Commands whose argument is not text.
            "label" | "vspace" | "hspace" | "setlength" | "pagestyle" | "thispagestyle" | "index" | "bibliographystyle" | "bibliography" | "input" | "include" | "setcounter" | "addtocounter" | "phantom" | "cline" | "usepackage" | "newcommand" | "renewcommand" | "DeclareMathOperator" => {
                let (_, spaced) = optional(spaced);
                group(spaced).map_or(consumed(tail), |(_, after)| consumed(after))
            }
            "item" => {
                out.push_str("• ");
                consumed(tail)
            }
            "LaTeX" | "TeX" => {
                out.push_str(name);
                consumed(spaced)
            }
            "ldots" | "dots" | "textellipsis" => {
                out.push('…');
                consumed(spaced)
            }
            "S" => {
                out.push('§');
                consumed(spaced)
            }
            "P" => {
                out.push('¶');
                consumed(spaced)
            }
            "copyright" => {
                out.push('©');
                consumed(spaced)
            }
            "textbullet" => {
                out.push('•');
                consumed(spaced)
            }
            "textbackslash" => {
                out.push_str("\\\\");
                consumed(spaced)
            }
            "quad" | "qquad" => {
                out.push_str("\u{2003}");
                consumed(spaced)
            }
            // Everything else loses its name and keeps its text; a style
            // declaration or spacing command simply vanishes.
            _ => consumed(spaced),
        }
    }

    /// One row of a table as Markdown, or None for a line of rules only.
    fn table_row(&mut self, t: &str) -> Option<String> {
        let mut row = t.to_string();
        for rule in ["\\hline", "\\toprule", "\\midrule", "\\bottomrule"] {
            row = row.replace(rule, "");
        }
        let row = row.trim();
        let row = row.strip_suffix("\\\\").unwrap_or(row).trim();
        if row.is_empty() || row.starts_with("\\cline") {
            return None;
        }
        let mut cells: Vec<String> = Vec::new();
        for cell in split_cells(row) {
            let cell = cell.trim();
            if let Some(rest) = cell.strip_prefix("\\multicolumn") {
                let Some((span, rest)) = group(rest) else { continue };
                let rest = group(rest).map_or(rest, |(_, after)| after);
                let text = group(rest).map_or("", |(g, _)| g);
                cells.push(self.text(text, true).trim().replace('|', "\\|"));
                for _ in 1..span.trim().parse::<usize>().unwrap_or(1).min(20) {
                    cells.push(String::new());
                }
            } else {
                cells.push(self.text(cell, true).trim().replace('|', "\\|"));
            }
        }
        Some(format!("| {} |", cells.join(" | ")))
    }
}

/// Cells of a table row, split at `&` outside math and not escaped.
fn split_cells(row: &str) -> Vec<&str> {
    let (mut cells, mut start, mut math, mut backslashes) = (Vec::new(), 0, false, 0);
    for (i, c) in row.char_indices() {
        match c {
            '$' if backslashes % 2 == 0 => math = !math,
            '&' if !math && backslashes % 2 == 0 => {
                cells.push(&row[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
    }
    cells.push(&row[start..]);
    cells
}

fn link(target: &str, label: &str) -> String {
    format!("[{label}]({})", target.trim().replace(' ', "%20").replace(')', "%29").replace('(', "%28"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(src: &str) -> String {
        to_markdown(src)
    }

    #[test]
    fn preamble_is_dropped_and_title_made() {
        let out = md("\\documentclass{article}\n\\title{On \\emph{Things}}\n\\author{A. Writer}\n\\begin{document}\n\\maketitle\n\\section{Intro}\nHello.\n\\end{document}\nignored\n");
        assert!(out.contains("# On *Things*"), "{out}");
        assert!(out.contains("*A. Writer*"), "{out}");
        assert!(out.contains("## Intro"), "{out}");
        assert!(!out.contains("documentclass") && !out.contains("ignored"), "{out}");
    }

    #[test]
    fn styles_and_escapes() {
        let out = md("Some \\textbf{bold \\textit{and}} text, 50\\% of a\\_b -- ok `` q ''.\n");
        assert_eq!(out.trim(), "Some **bold *and*** text, 50% of a\\_b – ok “ q ”.");
    }

    #[test]
    fn inline_math_survives_a_leading_digit() {
        assert_eq!(md("Let $2x_1 + y^2$ be.\n").trim(), "Let $`2x_1 + y^2`$ be.");
        assert_eq!(md("Again \\(a_b\\) here\n").trim(), "Again $`a_b`$ here");
    }

    #[test]
    fn display_math_becomes_a_math_fence() {
        let out = md("\\begin{equation}\n  E = mc^2 \\label{eq:e}\n\\end{equation}\n");
        assert_eq!(out, "```math\n  E = mc^2 \n```\n");
        let out = md("\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n");
        assert_eq!(out, "```math\n\\begin{aligned}a &= b \\\\\nc &= d\\end{aligned}\n```\n");
        assert_eq!(md("\\[ x^2 \\] done\n").trim(), "$$x^2$$ done");
    }

    #[test]
    fn lists_nest_and_number() {
        let out = md("\\begin{itemize}\n\\item one\n\\begin{enumerate}\n\\item deep\n\\end{enumerate}\n\\item two\n\\end{itemize}\n");
        assert_eq!(out, "\n- one\n  1. deep\n- two\n\n");
        let out = md("\\begin{description}\n\\item[Term] meaning\n\\end{description}\n");
        assert!(out.contains("- **Term** meaning"), "{out}");
    }

    #[test]
    fn verbatim_is_untouched() {
        let out = md("\\begin{lstlisting}[language=Python]\nx = 50% $y$ \\n\n\\end{lstlisting}\nafter\n");
        assert_eq!(out, "```python\nx = 50% $y$ \\n\n```\nafter\n");
    }

    #[test]
    fn tables_get_a_header_rule() {
        let out = md("\\begin{tabular}{ll}\n\\hline\nA & B \\\\ \\hline\n1 & $x$ \\\\\n\\end{tabular}\n");
        assert!(out.contains("| A | B |\n| --- | --- |\n| 1 | $`x`$ |"), "{out}");
    }

    #[test]
    fn links_citations_and_figures() {
        let out = md("See \\cite{knuth84}, \\href{https://x.org/a}{the site} and \\url{https://y.org}.\n\\includegraphics[width=3cm]{img/a b.png}\n");
        assert!(out.contains("\\[knuth84\\]") && out.contains("[the site](https://x.org/a)") && out.contains("[https://y.org](https://y.org)"), "{out}");
        assert!(out.contains("![](img/a%20b.png)"), "{out}");
    }

    #[test]
    fn comments_are_dropped_but_escaped_percent_is_text() {
        assert_eq!(md("100\\% sure % not this\n").trim(), "100% sure");
    }

    #[test]
    fn prose_that_looks_like_markdown_is_escaped() {
        assert_eq!(md("- not a list\n").trim(), "\\- not a list");
        assert_eq!(md("2. not either\n").trim(), "2\\. not either");
    }

    #[test]
    fn a_snippet_without_a_document_environment_is_all_body() {
        assert_eq!(md("\\section{Only}\ntext\n"), "## Only\ntext\n");
    }

    #[test]
    fn line_breaks() {
        assert_eq!(md("a \\\\\nb\n"), "a\\\nb\n");
        assert_eq!(md("a \\\\\n\nb\n"), "a\n\nb\n");
    }

    #[test]
    fn converted_math_reaches_katex_through_the_real_parser() {
        let src = "\\documentclass{article}\n\\begin{document}\n\\section{Math}\nLet $2x_1$ be, and\n\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n\\end{document}\n";
        let dir = std::path::Path::new("/tmp");
        let doc = crate::doc::Document::parse(to_markdown(src), &dir.join("a.tex"), dir);
        let html: String = doc.blocks.iter().map(|b| b.html.to_string()).collect();
        assert!(doc.blocks.iter().filter(|b| b.math).count() == 2, "{html}");
        assert!(html.contains("data-math-style=\"inline\"") && html.contains("data-math-style=\"display\""), "{html}");
        assert!(html.contains("\\begin{aligned}a &amp;= b"), "{html}");
        assert!(doc.blocks.iter().any(|b| b.heading.is_some()), "{html}");
    }
}
