//! Large-scale comparison with Shiki, on a corpus generated locally.
//!
//! Set `OXLIGHT_CORPUS` to a directory prepared with
//! `node scripts/fixtures/corpus.mjs <dir>`, then run
//! `cargo test --release --test corpus -- --nocapture`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use oxlight::{HighlightOptions, Highlighter};

const THEMES: &[&str] = &["github-dark", "dracula", "one-dark-pro"];

#[test]
fn corpus_matches_shiki() {
    let Ok(dir) = std::env::var("OXLIGHT_CORPUS") else {
        eprintln!("OXLIGHT_CORPUS is not set, skipping");
        return;
    };
    let dir = PathBuf::from(dir);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut h = Highlighter::new();
    for entry in fs::read_dir(fixtures.join("langs")).unwrap() {
        h.load_language_json(&fs::read_to_string(entry.unwrap().path()).unwrap())
            .unwrap();
    }
    for theme in THEMES {
        h.load_theme_json(
            &fs::read_to_string(fixtures.join(format!("themes/{theme}.json"))).unwrap(),
        )
        .unwrap();
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();
    let mut failures = Vec::new();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let stem = file.trim_end_matches(".txt");
        let lang = stem.split('.').nth(1).unwrap();
        let code = fs::read_to_string(&path).unwrap();
        for theme in THEMES {
            let start = Instant::now();
            let actual = h
                .code_to_html(&code, &HighlightOptions::new(lang, *theme))
                .unwrap();
            if theme == &THEMES[0] {
                eprintln!("{file}: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
            }
            let expected = fs::read_to_string(dir.join(format!("{stem}__{theme}.html"))).unwrap();
            if actual != expected {
                let line = expected
                    .lines()
                    .zip(actual.lines())
                    .position(|(e, a)| e != a)
                    .unwrap_or(0);
                failures.push(format!(
                    "{stem} / {theme}, line {}:\n  expected: {}\n  actual:   {}",
                    line + 1,
                    expected.lines().nth(line).unwrap_or(""),
                    actual.lines().nth(line).unwrap_or("")
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
