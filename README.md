# oxlight

Syntax highlighting with TextMate grammars and VS Code themes, in Rust.

oxlight is a port of [Shiki](https://shiki.style). It reads the same language
and theme JSON files as Shiki, and produces the same HTML, byte for byte.
It is the highlighting core of Oxmark, a Rust port
of Comark, and is built for speed. Bindings for other languages live in
separate projects.

```rust
use oxlight::{HighlightOptions, Highlighter};

let mut highlighter = Highlighter::new();
highlighter.load_language_json(&std::fs::read_to_string("rust.json")?)?;
highlighter.load_theme_json(&std::fs::read_to_string("github-dark.json")?)?;

let html = highlighter.code_to_html(
    "fn main() {}",
    &HighlightOptions::new("rust", "github-dark"),
)?;
// <pre class="shiki github-dark" style="background-color:#24292e;color:#e1e4e8" tabindex="0"><code>...
```

## Features

- **TextMate grammars**: a port of
  [vscode-textmate](https://github.com/microsoft/vscode-textmate) (begin/end
  and begin/while rules, captures, back references, `\A`/`\G` anchors,
  embedded grammars, injections), running on
  [Oniguruma](https://github.com/kkos/oniguruma), the regex engine the
  grammars are written for.
- **VS Code themes**: scope selectors with parent scopes, font styles, and
  Shiki's normalization (default colors, non-hex colors such as CSS variables,
  color replacements).
- **Shiki compatible output**: `code_to_html` and `code_to_tokens`, including
  multiple themes (light/dark) with CSS variables or `light-dark()`,
  whitespace merging and merging of same-style tokens.
- **Shareable**: a `Highlighter` is `Send + Sync`, compiles each grammar once,
  lazily, and caches compiled regexes.

## Languages and themes

oxlight does not bundle grammars or themes yet. Load them as JSON, in the
format of Shiki's registrations, from:

- [`tm-grammars`](https://github.com/shikijs/textmate-grammars-themes) and
  [`tm-themes`](https://github.com/shikijs/textmate-grammars-themes), or the
  `@shikijs/langs` and `@shikijs/themes` npm packages;
- any VS Code theme (`tokenColors` and `colors`) or TextMate grammar in JSON.

A language that embeds others (for example PHP, which embeds HTML, CSS,
JavaScript and SQL) needs those languages to be loaded too.

## Options

`HighlightOptions` mirrors Shiki's options:

| Shiki | oxlight |
| --- | --- |
| `lang`, `theme` | `HighlightOptions::new(lang, theme)` |
| `themes`, `defaultColor`, `cssVariablePrefix`, `colorsRendering` | `HighlightOptions::with_themes(lang, [...])`, `ThemeSelection::Multiple` |
| `colorReplacements` | `color_replacements` |
| `tokenizeMaxLineLength`, `tokenizeTimeLimit` | `tokenize_max_line_length`, `tokenize_time_limit` |
| `mergeWhitespaces`, `mergeSameStyleTokens` | `merge_whitespaces`, `merge_same_style_tokens` |
| `tabindex`, `rootStyle` | `tabindex`, `root_style` |
| `includeExplanation: 'scopeName'` | `include_scopes` |

## Differences with Shiki

- Token offsets are **UTF-8 byte offsets**, not UTF-16 offsets.
- A regex that Oniguruma cannot compile never matches, instead of failing the
  whole highlighting.
- Not ported yet: transformers and decorations, the `ansi` language, the
  `inline` structure, grammar state across calls, explanations with theme
  settings.

## Compatibility testing

`tests/golden.rs` compares the HTML of oxlight with the HTML of Shiki 3.23
for 10 samples, 9 languages and 8 themes, plus a light/dark rendering. The
fixtures are generated from the real Shiki:

```sh
cd scripts/fixtures
npm install
npm run generate
```

`tests/corpus.rs` runs the same comparison on any directory of files, prepared
with `node scripts/fixtures/corpus.mjs <dir>`:

```sh
OXLIGHT_CORPUS=<dir> cargo test --release --test corpus -- --nocapture
```

## Credits

oxlight translates code from [vscode-textmate](https://github.com/microsoft/vscode-textmate)
(© Microsoft Corporation) and [Shiki](https://github.com/shikijs/shiki)
(© Pine Wu, Anthony Fu), both under the MIT license. See [NOTICE](NOTICE).

Other Rust ports of Shiki were looked at: [`shiki`](https://github.com/cnlancehu/shiki)
(MIT OR Apache-2.0) and [`irosashi`](https://github.com/frostybee/irosashi) (MIT).

## License

[Apache License 2.0](LICENSE).
