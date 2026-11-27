//! Themed tokens, ported from Shiki's `codeToTokens` and its helpers.

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::grammar::StateStack;
use crate::highlighter::{is_plain_lang, Highlighter};
use crate::options::{
    ColorsRendering, DefaultColor, HighlightOptions, MergeWhitespaces, ThemeSelection,
};
use crate::theme::{font_style, Theme};

/// A token with its resolved style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemedToken {
    /// Text of the token.
    pub content: String,
    /// Byte offset of the token in the input.
    ///
    /// Shiki uses UTF-16 offsets; oxlight uses UTF-8 byte offsets.
    pub offset: usize,
    /// Foreground color, when the token has one.
    pub color: Option<String>,
    /// Background color, when the token has one.
    pub bg_color: Option<String>,
    /// Font style bit flags, see [`crate::font_style`].
    pub font_style: u8,
    /// Precomputed CSS declarations, used for multiple themes.
    pub html_style: Option<Vec<(String, String)>>,
    /// TextMate scopes of the token, when requested. With merged tokens,
    /// these are the scopes of the first grammar token.
    pub scopes: Option<Vec<String>>,
}

impl ThemedToken {
    /// CSS declarations of the token, in Shiki's order.
    pub fn style(&self) -> Vec<(String, String)> {
        if let Some(style) = &self.html_style {
            return style.clone();
        }
        let mut styles = Vec::new();
        if let Some(color) = self.color.as_ref().filter(|c| !c.is_empty()) {
            styles.push(("color".to_string(), color.clone()));
        }
        if let Some(bg) = self.bg_color.as_ref().filter(|c| !c.is_empty()) {
            styles.push(("background-color".to_string(), bg.clone()));
        }
        if self.font_style != 0 {
            if self.font_style & font_style::ITALIC != 0 {
                styles.push(("font-style".to_string(), "italic".to_string()));
            }
            if self.font_style & font_style::BOLD != 0 {
                styles.push(("font-weight".to_string(), "bold".to_string()));
            }
            let mut decorations = Vec::new();
            if self.font_style & font_style::UNDERLINE != 0 {
                decorations.push("underline");
            }
            if self.font_style & font_style::STRIKETHROUGH != 0 {
                decorations.push("line-through");
            }
            if !decorations.is_empty() {
                styles.push(("text-decoration".to_string(), decorations.join(" ")));
            }
        }
        styles
    }

    /// The `style` attribute value (`color:#fff;font-style:italic`).
    pub fn style_string(&self) -> String {
        stringify_style(&self.style())
    }

    fn is_decorated(&self) -> bool {
        self.font_style & (font_style::UNDERLINE | font_style::STRIKETHROUGH) != 0
    }
}

pub(crate) fn stringify_style(style: &[(String, String)]) -> String {
    style
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// The result of [`Highlighter::code_to_tokens`].
#[derive(Clone, Debug)]
pub struct TokensResult {
    /// Tokens, line by line.
    pub tokens: Vec<Vec<ThemedToken>>,
    /// Default foreground color (or CSS declarations, with multiple themes).
    pub fg: String,
    /// Default background color (or CSS declarations, with multiple themes).
    pub bg: String,
    /// Theme name(s), used as CSS classes.
    pub theme_name: String,
    /// Custom style of the root element, with multiple themes and no default color.
    pub root_style: Option<String>,
}

/// Port of Shiki's `splitLines` (without line endings): `(line, offset)`.
pub(crate) fn split_lines(code: &str) -> Vec<(&str, usize)> {
    if code.is_empty() {
        return vec![("", 0)];
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let end = if i > start && bytes[i - 1] == b'\r' {
                i - 1
            } else {
                i
            };
            lines.push((&code[start..end], start));
            start = i + 1;
        }
        i += 1;
    }
    lines.push((&code[start..], start));
    lines
}

/// Tokens, line by line.
type Lines = Vec<Vec<ThemedToken>>;

/// Tokenizes with one theme. Port of `codeToTokensBase` / `_tokenizeWithTheme`.
pub(crate) fn tokenize_with_theme(
    highlighter: &Highlighter,
    code: &str,
    lang: &str,
    theme: &Theme,
    options: &HighlightOptions,
) -> Result<(Lines, Option<Arc<StateStack>>)> {
    let lang = highlighter.resolve_alias(lang);
    if is_plain_lang(&lang) {
        return Ok((plain_tokens(code), None));
    }
    let grammar = highlighter.grammar(&lang)?;
    let tm = &theme.textmate;
    let color_map = tm.color_map();
    let mut state: Option<Arc<StateStack>> = None;
    let mut out = Vec::new();
    for (line, line_offset) in split_lines(code) {
        if line.is_empty() {
            out.push(Vec::new());
            continue;
        }
        if options.tokenize_max_line_length > 0
            && line.encode_utf16().count() >= options.tokenize_max_line_length
        {
            out.push(vec![ThemedToken {
                content: line.to_string(),
                offset: line_offset,
                color: Some(String::new()),
                ..Default::default()
            }]);
            continue;
        }
        let result = grammar.tokenize_line(
            tm,
            line,
            state.as_ref(),
            options.tokenize_time_limit,
            options.include_scopes,
        );
        let mut tokens = Vec::with_capacity(result.tokens.len());
        for (j, token) in result.tokens.iter().enumerate() {
            let start = token.start;
            let next = result.tokens.get(j + 1).map_or(line.len(), |t| t.start);
            if start == next {
                continue;
            }
            // A token can end beyond the line when the grammar matched the
            // line ending, which is not part of the line here.
            let end = next.min(line.len());
            if start >= end {
                continue;
            }
            let color = color_map
                .get(token.attributes.foreground as usize)
                .map(|c| {
                    theme
                        .replace_color(c, &options.color_replacements)
                        .to_string()
                });
            tokens.push(ThemedToken {
                content: line[start..end].to_string(),
                offset: line_offset + start,
                color,
                font_style: token.attributes.font_style,
                scopes: token.scopes.clone(),
                ..Default::default()
            });
        }
        out.push(tokens);
        state = Some(result.state);
    }
    Ok((out, state))
}

fn plain_tokens(code: &str) -> Vec<Vec<ThemedToken>> {
    split_lines(code)
        .into_iter()
        .map(|(line, offset)| {
            vec![ThemedToken {
                content: line.to_string(),
                offset,
                ..Default::default()
            }]
        })
        .collect()
}

/// Port of Shiki's `codeToTokens`, with whitespace merging from `codeToHast`.
pub(crate) fn code_to_tokens(
    highlighter: &Highlighter,
    code: &str,
    options: &HighlightOptions,
) -> Result<TokensResult> {
    let mut result = match &options.theme {
        ThemeSelection::Single(name) => {
            let theme = highlighter.theme_arc(name)?;
            let (tokens, _) =
                tokenize_with_theme(highlighter, code, &options.lang, &theme, options)?;
            TokensResult {
                tokens,
                fg: theme
                    .replace_color(&theme.fg, &options.color_replacements)
                    .to_string(),
                bg: theme
                    .replace_color(&theme.bg, &options.color_replacements)
                    .to_string(),
                theme_name: theme.name.clone(),
                root_style: None,
            }
        }
        ThemeSelection::Multiple {
            themes,
            default_color,
            css_variable_prefix,
            colors_rendering,
        } => multi_theme_tokens(
            highlighter,
            code,
            options,
            themes,
            default_color,
            css_variable_prefix,
            *colors_rendering,
        )?,
    };
    result.tokens = match options.merge_whitespaces {
        MergeWhitespaces::Merge => merge_whitespace_tokens(result.tokens),
        MergeWhitespaces::Never => split_whitespace_tokens(result.tokens),
        MergeWhitespaces::Keep => result.tokens,
    };
    if options.merge_same_style_tokens {
        result.tokens = merge_adjacent_styled_tokens(result.tokens);
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn multi_theme_tokens(
    highlighter: &Highlighter,
    code: &str,
    options: &HighlightOptions,
    themes: &[(String, String)],
    default_color: &DefaultColor,
    prefix: &str,
    rendering: ColorsRendering,
) -> Result<TokensResult> {
    let mut sorted: Vec<&(String, String)> = themes.iter().filter(|(_, t)| !t.is_empty()).collect();
    if sorted.is_empty() {
        return Err(Error::InvalidOptions("`themes` must not be empty".into()));
    }
    if let DefaultColor::Key(key) = default_color {
        if !sorted.iter().any(|(c, _)| c == key) {
            return Err(Error::InvalidOptions(format!(
                "`themes` must contain the default color key `{key}`"
            )));
        }
        // Stable sort that moves the default key first.
        sorted.sort_by_key(|(c, _)| c != key);
    }
    if *default_color == DefaultColor::LightDark && sorted.len() > 1 {
        let has = |k: &str| sorted.iter().any(|(c, _)| c == k);
        if !has("light") || !has("dark") {
            return Err(Error::InvalidOptions(
                "`light-dark()` needs both `light` and `dark` themes".into(),
            ));
        }
    }
    let resolved: Vec<Arc<Theme>> = sorted
        .iter()
        .map(|(_, name)| highlighter.theme_arc(name))
        .collect::<Result<_>>()?;
    let per_theme: Vec<Vec<Vec<ThemedToken>>> = resolved
        .iter()
        .map(|theme| {
            tokenize_with_theme(highlighter, code, &options.lang, theme, options).map(|r| r.0)
        })
        .collect::<Result<_>>()?;
    let synced = sync_themes_tokenization(per_theme);
    let order: Vec<&str> = sorted.iter().map(|(c, _)| c.as_str()).collect();

    let line_count = synced[0].len();
    let mut tokens = Vec::with_capacity(line_count);
    for line in 0..line_count {
        let mut out_line = Vec::with_capacity(synced[0][line].len());
        for (idx, first) in synced[0][line].iter().enumerate() {
            let variants: Vec<&ThemedToken> = synced.iter().map(|t| &t[line][idx]).collect();
            out_line.push(ThemedToken {
                content: first.content.clone(),
                offset: first.offset,
                html_style: Some(flat_token_variants(
                    &variants,
                    &order,
                    prefix,
                    default_color,
                    rendering,
                )),
                scopes: first.scopes.clone(),
                ..Default::default()
            });
        }
        tokens.push(out_line);
    }

    let replacements = &options.color_replacements;
    let map_colors = |fg: bool| -> String {
        let value_of = |i: usize| {
            let t = &resolved[i];
            let v = if fg { &t.fg } else { &t.bg };
            let v = t.replace_color(v, replacements);
            if v.is_empty() {
                "inherit".to_string()
            } else {
                v.to_string()
            }
        };
        let mut parts = Vec::new();
        for (i, color) in order.iter().enumerate() {
            let value = value_of(i);
            let var = format!("{prefix}{color}{}:{value}", if fg { "" } else { "-bg" });
            if i == 0 && *default_color != DefaultColor::None {
                if *default_color == DefaultColor::LightDark && order.len() > 1 {
                    let li = order.iter().position(|c| *c == "light").unwrap_or(0);
                    let di = order.iter().position(|c| *c == "dark").unwrap_or(0);
                    parts.push(format!(
                        "light-dark({}, {});{var}",
                        value_of(li),
                        value_of(di)
                    ));
                } else {
                    parts.push(value);
                }
                continue;
            }
            if rendering == ColorsRendering::CssVars {
                parts.push(var);
            }
        }
        parts.join(";")
    };
    let fg = map_colors(true);
    let bg = map_colors(false);
    let theme_name = format!(
        "shiki-themes {}",
        resolved
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let root_style = (*default_color == DefaultColor::None).then(|| format!("{fg};{bg}"));
    Ok(TokensResult {
        tokens,
        fg,
        bg,
        theme_name,
        root_style,
    })
}

/// Port of `flatTokenVariants`.
fn flat_token_variants(
    variants: &[&ThemedToken],
    order: &[&str],
    prefix: &str,
    default_color: &DefaultColor,
    rendering: ColorsRendering,
) -> Vec<(String, String)> {
    let styles: Vec<Vec<(String, String)>> = variants.iter().map(|t| t.style()).collect();
    let mut keys: Vec<String> = Vec::new();
    for style in &styles {
        for (k, _) in style {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
    }
    let get = |style: &[(String, String)], key: &str| -> String {
        style
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "inherit".to_string())
    };
    let var_key = |idx: usize, key: &str| -> String {
        let suffix = match key {
            "color" => String::new(),
            "background-color" => "-bg".to_string(),
            other => format!("-{other}"),
        };
        format!("{prefix}{}{suffix}", order[idx])
    };
    let mut merged: Vec<(String, String)> = Vec::new();
    let mut set = |k: String, v: String| {
        if let Some(entry) = merged.iter_mut().find(|(key, _)| *key == k) {
            entry.1 = v;
        } else {
            merged.push((k, v));
        }
    };
    for (idx, style) in styles.iter().enumerate() {
        for key in &keys {
            let value = get(style, key);
            let is_color_key = key == "color" || key == "background-color";
            if idx == 0 && *default_color != DefaultColor::None && is_color_key {
                if *default_color == DefaultColor::LightDark && styles.len() > 1 {
                    let li = order.iter().position(|c| *c == "light").unwrap_or(0);
                    let di = order.iter().position(|c| *c == "dark").unwrap_or(0);
                    set(
                        key.clone(),
                        format!(
                            "light-dark({}, {})",
                            get(&styles[li], key),
                            get(&styles[di], key)
                        ),
                    );
                    if rendering == ColorsRendering::CssVars {
                        set(var_key(idx, key), value);
                    }
                } else {
                    set(key.clone(), value);
                }
            } else if rendering == ColorsRendering::CssVars {
                set(var_key(idx, key), value);
            }
        }
    }
    merged
}

/// Port of `syncThemesTokenization`: splits tokens so that every theme has the
/// same token boundaries.
fn sync_themes_tokenization(themes: Vec<Vec<Vec<ThemedToken>>>) -> Vec<Vec<Vec<ThemedToken>>> {
    let count = themes.len();
    let mut out: Vec<Vec<Vec<ThemedToken>>> = vec![Vec::new(); count];
    let line_count = themes[0].len();
    for i in 0..line_count {
        let lines: Vec<&Vec<ThemedToken>> = themes.iter().map(|t| &t[i]).collect();
        let mut out_lines: Vec<Vec<ThemedToken>> = vec![Vec::new(); count];
        let mut indexes = vec![0usize; count];
        let mut current: Vec<Option<ThemedToken>> =
            lines.iter().map(|l| l.first().cloned()).collect();
        while current.iter().all(Option::is_some) {
            let min = current
                .iter()
                .map(|t| t.as_ref().map_or(0, |t| t.content.len()))
                .min()
                .unwrap_or(0);
            for n in 0..count {
                let token = current[n].take().unwrap_or_default();
                if token.content.len() == min {
                    out_lines[n].push(token);
                    indexes[n] += 1;
                    current[n] = lines[n].get(indexes[n]).cloned();
                } else {
                    let mut head = token.clone();
                    head.content = token.content[..min].to_string();
                    out_lines[n].push(head);
                    let mut tail = token.clone();
                    tail.content = token.content[min..].to_string();
                    tail.offset = token.offset + min;
                    current[n] = Some(tail);
                }
            }
        }
        for (n, line) in out_lines.into_iter().enumerate() {
            out[n].push(line);
        }
    }
    out
}

fn is_js_whitespace_only(s: &str) -> bool {
    !s.is_empty() && s.chars().all(crate::grammar::is_js_whitespace)
}

/// Port of `mergeWhitespaceTokens`.
fn merge_whitespace_tokens(lines: Vec<Vec<ThemedToken>>) -> Vec<Vec<ThemedToken>> {
    lines
        .into_iter()
        .map(|line| {
            let mut out = Vec::with_capacity(line.len());
            let mut carry = String::new();
            let mut first_offset: Option<usize> = None;
            let len = line.len();
            for (idx, token) in line.into_iter().enumerate() {
                let could_merge = !token.is_decorated();
                if could_merge && is_js_whitespace_only(&token.content) && idx + 1 < len {
                    first_offset.get_or_insert(token.offset);
                    carry.push_str(&token.content);
                } else if !carry.is_empty() {
                    let offset = first_offset.take().unwrap_or(token.offset);
                    if could_merge {
                        let mut merged = token;
                        merged.content = std::mem::take(&mut carry) + &merged.content;
                        merged.offset = offset;
                        out.push(merged);
                    } else {
                        out.push(ThemedToken {
                            content: std::mem::take(&mut carry),
                            offset,
                            ..Default::default()
                        });
                        out.push(token);
                    }
                } else {
                    out.push(token);
                }
            }
            out
        })
        .collect()
}

/// Port of `splitWhitespaceTokens`.
fn split_whitespace_tokens(lines: Vec<Vec<ThemedToken>>) -> Vec<Vec<ThemedToken>> {
    lines
        .into_iter()
        .map(|line| {
            let mut out = Vec::with_capacity(line.len());
            for token in line {
                if is_js_whitespace_only(&token.content) {
                    out.push(token);
                    continue;
                }
                let content = token.content.as_str();
                let trimmed_start = content.trim_start_matches(crate::grammar::is_js_whitespace);
                let leading = content.len() - trimmed_start.len();
                let middle = trimmed_start.trim_end_matches(crate::grammar::is_js_whitespace);
                let trailing = trimmed_start.len() - middle.len();
                if leading == 0 && trailing == 0 {
                    out.push(token);
                    continue;
                }
                if leading > 0 {
                    out.push(ThemedToken {
                        content: content[..leading].to_string(),
                        offset: token.offset,
                        ..Default::default()
                    });
                }
                let mut mid = token.clone();
                mid.content = middle.to_string();
                mid.offset = token.offset + leading;
                out.push(mid);
                if trailing > 0 {
                    out.push(ThemedToken {
                        content: content[content.len() - trailing..].to_string(),
                        offset: token.offset + leading + middle.len(),
                        ..Default::default()
                    });
                }
            }
            out
        })
        .collect()
}

/// Port of `mergeAdjacentStyledTokens`.
fn merge_adjacent_styled_tokens(lines: Vec<Vec<ThemedToken>>) -> Vec<Vec<ThemedToken>> {
    lines
        .into_iter()
        .map(|line| {
            let mut out: Vec<ThemedToken> = Vec::with_capacity(line.len());
            for token in line {
                if let Some(prev) = out.last_mut() {
                    if !prev.is_decorated()
                        && !token.is_decorated()
                        && prev.style_string() == token.style_string()
                    {
                        prev.content.push_str(&token.content);
                        continue;
                    }
                }
                out.push(token);
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lines_like_shiki() {
        assert_eq!(split_lines(""), vec![("", 0)]);
        assert_eq!(split_lines("a\r\nb\n"), vec![("a", 0), ("b", 3), ("", 5)]);
        assert_eq!(split_lines("a\rb"), vec![("a\rb", 0)]);
    }
}
