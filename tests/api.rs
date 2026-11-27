//! Behaviour of the public API.

use std::fs;
use std::path::Path;

use oxlight::{font_style, Error, HighlightOptions, Highlighter, MergeWhitespaces};

fn fixture(path: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(path),
    )
    .unwrap()
}

fn rust_highlighter() -> Highlighter {
    let mut h = Highlighter::new();
    h.load_language_json(&fixture("langs/rust.json")).unwrap();
    h.load_theme_json(&fixture("themes/github-dark.json"))
        .unwrap();
    h
}

#[test]
fn plain_text_is_escaped_without_colors() {
    let h = rust_highlighter();
    let html = h
        .code_to_html(
            "a < b && c\n",
            &HighlightOptions::new("text", "github-dark"),
        )
        .unwrap();
    assert_eq!(
        html,
        "<pre class=\"shiki github-dark\" style=\"background-color:#24292e;color:#e1e4e8\" tabindex=\"0\"><code>\
         <span class=\"line\"><span>a &#x3C; b &#x26;&#x26; c</span></span>\n<span class=\"line\"><span></span></span></code></pre>"
    );
}

#[test]
fn unknown_language_and_theme_are_errors() {
    let h = rust_highlighter();
    let err = h
        .code_to_html("x", &HighlightOptions::new("cobol", "github-dark"))
        .unwrap_err();
    assert!(matches!(err, Error::UnknownLanguage(ref l) if l == "cobol"));
    let err = h
        .code_to_html("x", &HighlightOptions::new("rust", "nope"))
        .unwrap_err();
    assert!(matches!(err, Error::UnknownTheme(_)));
}

#[test]
fn aliases_resolve_to_languages() {
    let h = rust_highlighter();
    assert_eq!(h.language("rs").map(|l| l.name.as_str()), Some("rust"));
    let a = h
        .code_to_html("fn x() {}", &HighlightOptions::new("rs", "github-dark"))
        .unwrap();
    let b = h
        .code_to_html("fn x() {}", &HighlightOptions::new("rust", "github-dark"))
        .unwrap();
    assert_eq!(a, b);
}

#[test]
fn tokens_have_byte_offsets_and_styles() {
    let h = rust_highlighter();
    let mut options = HighlightOptions::new("rust", "github-dark");
    options.include_scopes = true;
    let result = h.code_to_tokens("let é = 1;\nfn f() {}", &options).unwrap();
    assert_eq!(result.fg, "#e1e4e8");
    assert_eq!(result.tokens.len(), 2);
    let line = &result.tokens[0];
    assert_eq!(line[0].content, "let");
    assert_eq!(line[0].color.as_deref(), Some("#F97583"));
    let reconstructed: String = line.iter().map(|t| t.content.as_str()).collect();
    assert_eq!(reconstructed, "let é = 1;");
    let one = line.iter().find(|t| t.content.trim() == "1").unwrap();
    assert_eq!(
        &"let é = 1;\nfn f() {}"[one.offset..one.offset + one.content.len()],
        one.content
    );
    assert_eq!(result.tokens[1][0].offset, 12); // "é" is two bytes
    let scopes = line[0].scopes.as_ref().unwrap();
    assert_eq!(scopes[0], "source.rust");
    assert!(scopes.iter().any(|s| s.starts_with("keyword")));
}

#[test]
fn whitespace_merging_modes() {
    let h = rust_highlighter();
    let mut options = HighlightOptions::new("rust", "github-dark");
    let merged = h.code_to_tokens("let  x", &options).unwrap();
    options.merge_whitespaces = MergeWhitespaces::Never;
    let split = h.code_to_tokens("let  x", &options).unwrap();
    assert!(merged.tokens[0].iter().any(|t| t.content == "  x"));
    assert!(split.tokens[0].iter().any(|t| t.content == "  "));
    assert!(split.tokens[0].iter().any(|t| t.content == "x"));
}

#[test]
fn vscode_theme_with_non_hex_colors() {
    let mut h = rust_highlighter();
    h.load_theme_json(
        r#"{
          "name": "vars",
          "type": "light",
          "colors": { "editor.foreground": "var(--fg)", "editor.background": "var(--bg)" },
          "tokenColors": [
            { "scope": "keyword", "settings": { "foreground": "var(--kw)", "fontStyle": "bold underline" } }
          ]
        }"#,
    )
    .unwrap();
    let result = h
        .code_to_tokens("let x", &HighlightOptions::new("rust", "vars"))
        .unwrap();
    assert_eq!(result.fg, "var(--fg)");
    assert_eq!(result.bg, "var(--bg)");
    let kw = &result.tokens[0][0];
    assert_eq!(kw.content, "let");
    assert_eq!(kw.color.as_deref(), Some("var(--kw)"));
    assert_eq!(kw.font_style, font_style::BOLD | font_style::UNDERLINE);
    assert_eq!(
        kw.style_string(),
        "color:var(--kw);font-weight:bold;text-decoration:underline"
    );
}

#[test]
fn color_replacements_option() {
    let h = rust_highlighter();
    let mut options = HighlightOptions::new("rust", "github-dark");
    options
        .color_replacements
        .insert("#f97583".into(), "red".into());
    let result = h.code_to_tokens("let x", &options).unwrap();
    assert_eq!(result.tokens[0][0].color.as_deref(), Some("red"));
}

#[test]
fn highlighter_can_be_shared_between_threads() {
    let h = std::sync::Arc::new(rust_highlighter());
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let h = h.clone();
            std::thread::spawn(move || {
                h.code_to_html(
                    &format!("fn f{i}() {{ {i} }}"),
                    &HighlightOptions::new("rust", "github-dark"),
                )
                .unwrap()
            })
        })
        .collect();
    for (i, handle) in handles.into_iter().enumerate() {
        assert!(handle.join().unwrap().contains(&format!("f{i}")));
    }
}
