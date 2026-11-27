//! Highlights a file to HTML.
//!
//! ```sh
//! cargo run --example highlight -- <language.json> <theme.json> <lang> <file>
//! ```

use std::{env, fs, process};

use oxlight::{HighlightOptions, Highlighter};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: highlight <language.json>... <theme.json> <lang> <file>");
        process::exit(2);
    }
    let (file, lang, theme) = (
        &args[args.len() - 1],
        &args[args.len() - 2],
        &args[args.len() - 3],
    );
    let mut highlighter = Highlighter::new();
    for path in &args[..args.len() - 3] {
        highlighter.load_language_json(&fs::read_to_string(path)?)?;
    }
    let theme_name = highlighter.load_theme_json(&fs::read_to_string(theme)?)?;
    let code = fs::read_to_string(file)?;
    let html =
        highlighter.code_to_html(&code, &HighlightOptions::new(lang.as_str(), theme_name))?;
    println!("{html}");
    Ok(())
}
