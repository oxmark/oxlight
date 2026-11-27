//! # oxlight
//!
//! Syntax highlighting with TextMate grammars and VS Code themes: a Rust port
//! of [Shiki](https://shiki.style).
//!
//! oxlight reads the same language and theme JSON files as Shiki and aims to
//! produce the same HTML, byte for byte.
//!
//! ```no_run
//! use oxlight::{Highlighter, HighlightOptions};
//!
//! let mut highlighter = Highlighter::new();
//! highlighter.load_language_json(&std::fs::read_to_string("rust.json")?)?;
//! highlighter.load_theme_json(&std::fs::read_to_string("github-dark.json")?)?;
//!
//! let html = highlighter.code_to_html(
//!     "fn main() {}",
//!     &HighlightOptions::new("rust", "github-dark"),
//! )?;
//! assert!(html.starts_with("<pre class=\"shiki github-dark\""));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The tokenizer is a port of
//! [vscode-textmate](https://github.com/microsoft/vscode-textmate) and uses
//! [Oniguruma](https://github.com/kkos/oniguruma), the regex engine that
//! TextMate grammars are written for.

#![warn(missing_docs)]

mod error;
mod grammar;
mod highlighter;
mod html;
mod language;
mod options;
mod regex;
mod theme;
mod tokens;

pub use error::{Error, Result};
pub use highlighter::Highlighter;
pub use language::Language;
pub use options::{
    ColorsRendering, DefaultColor, HighlightOptions, MergeWhitespaces, ThemeSelection,
};
pub use theme::{font_style, Theme, ThemeScope, ThemeSetting, ThemeStyle};
pub use tokens::{ThemedToken, TokensResult};
