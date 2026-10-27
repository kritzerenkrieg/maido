//! Code highlighting with `syntect` (H4): by the fence's language only (B4),
//! cached by a hash of language and code, so unchanged code is highlighted once.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

const STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };
/// Entries kept before the cache starts over; a highlighted block is a few KB.
const CACHE_LIMIT: usize = 2000;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static CACHE: LazyLock<Mutex<HashMap<blake3::Hash, Option<String>>>> = LazyLock::new(Default::default);

/// The inner HTML of `<code>` for `code` in `lang`, or None for a language
/// syntect does not know (the block stays plain).
pub fn highlight(lang: &str, code: &str) -> Option<String> {
    let key = blake3::hash(format!("{lang}\0{code}").as_bytes());
    if let Some(hit) = CACHE.lock().unwrap().get(&key) {
        return hit.clone();
    }
    let html = render(lang, code);
    let mut cache = CACHE.lock().unwrap();
    if cache.len() >= CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(key, html.clone());
    html
}

fn render(lang: &str, code: &str) -> Option<String> {
    let lang = match lang.to_ascii_lowercase().as_str() {
        // Not in syntect's default set; JavaScript's grammar is close enough.
        "ts" | "typescript" | "tsx" | "jsx" | "mjs" | "cjs" => "js".to_string(),
        "shell" | "console" | "zsh" => "sh".to_string(),
        "yml" => "yaml".to_string(),
        l => l.to_string(),
    };
    let syntax = SYNTAXES.find_syntax_by_token(&lang)?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, &SYNTAXES, STYLE);
    for line in LinesWithEndings::from(code) {
        generator.parse_html_for_line_which_includes_newline(line).ok()?;
    }
    Some(generator.finalize())
}

/// Source files larger than this open as plain text: highlighting a whole
/// file runs at a few MB a second.
const MAX_CODE: usize = 1024 * 1024;

/// The language of a source file, by its extension or name: syntect's token
/// for it and the name shown. None for anything else (plain text).
pub fn language(path: &Path) -> Option<(&'static str, &'static str)> {
    let name = path.file_name()?.to_str()?;
    match name {
        "Makefile" | "makefile" | "GNUmakefile" => return Some(("make", "Makefile")),
        "CMakeLists.txt" => return None,
        _ => {}
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => ("rs", "Rust"),
        "py" | "pyw" | "pyi" => ("py", "Python"),
        "c" | "h" => ("c", "C"),
        "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "ino" => ("cpp", "C++"),
        "cs" => ("cs", "C#"),
        "java" => ("java", "Java"),
        "go" => ("go", "Go"),
        "php" => ("php", "PHP"),
        "json" | "jsonc" | "json5" => ("json", "JSON"),
        "js" | "mjs" | "cjs" | "jsx" => ("js", "JavaScript"),
        // Not in syntect's default set; JavaScript's grammar is close enough.
        "ts" | "mts" | "cts" | "tsx" => ("js", "TypeScript"),
        "sh" | "bash" | "zsh" => ("sh", "Shell"),
        "rb" => ("rb", "Ruby"),
        "lua" => ("lua", "Lua"),
        "pl" | "pm" => ("pl", "Perl"),
        "r" => ("r", "R"),
        "scala" => ("scala", "Scala"),
        "hs" => ("hs", "Haskell"),
        "ml" | "mli" => ("ml", "OCaml"),
        "erl" => ("erl", "Erlang"),
        "clj" => ("clj", "Clojure"),
        "d" => ("d", "D"),
        "m" => ("m", "Objective-C"),
        "html" | "htm" => ("html", "HTML"),
        "css" => ("css", "CSS"),
        "xml" | "xsd" | "xsl" | "svg" => ("xml", "XML"),
        "yaml" | "yml" => ("yaml", "YAML"),
        "sql" => ("sql", "SQL"),
        "bat" | "cmd" => ("bat", "Batch"),
        _ => return None,
    })
}

/// A whole source file as the inner HTML of `<pre>`, or None when it is too
/// large or syntect fails on it. Not cached: files change.
pub fn highlight_file(token: &str, code: &str) -> Option<String> {
    if code.len() > MAX_CODE {
        return None;
    }
    render(token, code)
}

/// Stylesheet for the `hl-` classes: a light theme, and a dark one when the
/// theme setting is Dark, or System with a dark desktop (L4).
pub fn css() -> String {
    let themes = ThemeSet::load_defaults();
    let light = css_for_theme_with_class_style(&themes.themes["InspiredGitHub"], STYLE).unwrap_or_default();
    let dark = css_for_theme_with_class_style(&themes.themes["base16-ocean.dark"], STYLE).unwrap_or_default();
    let system = scope(&dark, ":root:not([data-theme=\"light\"])");
    let chosen = scope(&dark, ":root[data-theme=\"dark\"]");
    // Screen only: printed code is always light.
    format!("{light}\n@media screen and (prefers-color-scheme: dark) {{\n{system}\n}}\n@media screen {{\n{chosen}\n}}\n")
}

/// Put `prefix` before every selector of every rule in `css` (one rule head per line).
fn scope(css: &str, prefix: &str) -> String {
    css.lines()
        .map(|line| match line.trim_end().strip_suffix('{') {
            Some(head) if !line.starts_with(char::is_whitespace) && !line.starts_with('@') => {
                let selectors: Vec<String> = head.split(',').map(|s| format!("{prefix} {}", s.trim())).collect();
                format!("{} {{\n", selectors.join(", "))
            }
            _ => format!("{line}\n"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_and_unknown_languages() {
        let html = super::highlight("rust", "fn main() { let x = \"<b>\"; }\n").unwrap();
        assert!(html.contains("hl-") && html.contains("&lt;b&gt;") && !html.contains("<b>"), "{html}");
        assert!(super::highlight("no-such-language", "x").is_none());
        assert!(super::highlight("ts", "const x: number = 1\n").is_some());
    }

    #[test]
    fn source_files() {
        use std::path::Path;
        for name in [
            "a.rs", "a.py", "a.c", "a.h", "a.cpp", "a.hpp", "a.cs", "a.java", "a.go", "a.php", "a.json", "a.js", "a.ts", "a.sh", "a.rb", "a.lua", "a.pl", "a.r", "a.scala",
            "a.hs", "a.ml", "a.erl", "a.clj", "a.d", "a.m", "a.html", "a.css", "a.xml", "a.yaml", "a.sql", "a.bat", "Makefile",
        ] {
            let (token, _) = super::language(Path::new(name)).unwrap_or_else(|| panic!("{name}"));
            assert!(super::SYNTAXES.find_syntax_by_token(token).is_some(), "{name}: no syntax {token}");
        }
        assert_eq!(super::language(Path::new("src/Main.JAVA")), Some(("java", "Java")));
        assert_eq!(super::language(Path::new("notes.txt")), None);
        assert_eq!(super::language(Path::new("README")), None);
        let html = super::highlight_file("py", "def f():\n    return '<x>'\n").unwrap();
        assert!(html.contains("hl-") && html.contains("&lt;x&gt;"), "{html}");
        assert!(super::highlight_file("py", &"x\n".repeat(600_000)).is_none());
    }

    #[test]
    fn dark_rules_scoped() {
        let css = super::css();
        let (light, rest) = css.split_once("@media screen").unwrap();
        assert!(light.contains(".hl-code {") && !light.contains(":root"));
        assert!(rest.contains(":root:not([data-theme=\"light\"]) .hl-code {"), "{rest}");
        assert!(rest.contains(":root[data-theme=\"dark\"] .hl-code {"), "{rest}");
        assert!(!rest.lines().any(|l| l.starts_with(".hl-")), "{rest}");
    }
}
