//! Compiled rules, ported from `rule.ts` of vscode-textmate.

use crate::regex::CaptureIndex;

/// Identifier of a compiled rule inside a grammar.
pub(crate) type RuleId = i32;

/// Rule id used by scanners for the `end` pattern of the current rule.
pub(crate) const END_RULE_ID: RuleId = -1;
/// Rule id used by scanners for the `while` pattern of the current rule.
pub(crate) const WHILE_RULE_ID: RuleId = -2;

/// A compiled rule.
#[derive(Debug)]
pub(crate) struct Rule {
    pub name: Option<String>,
    name_is_capturing: bool,
    pub content_name: Option<String>,
    content_name_is_capturing: bool,
    pub kind: RuleKind,
}

#[derive(Debug)]
pub(crate) enum RuleKind {
    Capture {
        /// Rule used to re-tokenize the captured text (0 when none).
        retokenize_with: RuleId,
    },
    Match {
        regex: RegExpSource,
        captures: Vec<Option<RuleId>>,
    },
    IncludeOnly {
        patterns: Vec<RuleId>,
        has_missing_patterns: bool,
    },
    BeginEnd {
        begin: RegExpSource,
        begin_captures: Vec<Option<RuleId>>,
        end: RegExpSource,
        end_captures: Vec<Option<RuleId>>,
        apply_end_pattern_last: bool,
        patterns: Vec<RuleId>,
        has_missing_patterns: bool,
    },
    BeginWhile {
        begin: RegExpSource,
        begin_captures: Vec<Option<RuleId>>,
        while_: RegExpSource,
        while_captures: Vec<Option<RuleId>>,
        patterns: Vec<RuleId>,
        has_missing_patterns: bool,
    },
}

impl Rule {
    pub fn new(name: Option<String>, content_name: Option<String>, kind: RuleKind) -> Self {
        Self {
            name_is_capturing: name.as_deref().is_some_and(has_captures),
            name,
            content_name_is_capturing: content_name.as_deref().is_some_and(has_captures),
            content_name,
            kind,
        }
    }

    /// The scope name, with `$1` style references replaced by captured text.
    pub fn get_name(
        &self,
        line: Option<&str>,
        captures: Option<&[CaptureIndex]>,
    ) -> Option<String> {
        match (&self.name, line, captures) {
            (Some(name), Some(line), Some(captures)) if self.name_is_capturing => {
                Some(replace_captures(name, line, captures))
            }
            (name, _, _) => name.clone(),
        }
    }

    /// The content scope name, with `$1` style references replaced.
    pub fn get_content_name(&self, line: &str, captures: &[CaptureIndex]) -> Option<String> {
        match &self.content_name {
            Some(name) if self.content_name_is_capturing => {
                Some(replace_captures(name, line, captures))
            }
            name => name.clone(),
        }
    }

    pub fn patterns(&self) -> Option<(&[RuleId], bool)> {
        match &self.kind {
            RuleKind::IncludeOnly {
                patterns,
                has_missing_patterns,
            }
            | RuleKind::BeginEnd {
                patterns,
                has_missing_patterns,
                ..
            }
            | RuleKind::BeginWhile {
                patterns,
                has_missing_patterns,
                ..
            } => Some((patterns, *has_missing_patterns)),
            _ => None,
        }
    }
}

/// `$n` or `${n:/downcase}` / `${n:/upcase}` references in scope names.
fn has_captures(source: &str) -> bool {
    let bytes = source.as_bytes();
    (0..bytes.len()).any(|i| parse_capture_ref(source, i).is_some())
}

/// Parses a capture reference at byte `i`, returning `(end, group, command)`.
fn parse_capture_ref(source: &str, i: usize) -> Option<(usize, usize, Option<&'static str>)> {
    let bytes = source.as_bytes();
    if bytes.get(i) != Some(&b'$') {
        return None;
    }
    let digits_at = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let n = digits_at(i + 1);
    if n > 0 {
        let group = source[i + 1..i + 1 + n].parse().ok()?;
        return Some((i + 1 + n, group, None));
    }
    if bytes.get(i + 1) == Some(&b'{') {
        let n = digits_at(i + 2);
        if n == 0 {
            return None;
        }
        let group = source[i + 2..i + 2 + n].parse().ok()?;
        let rest = &source[i + 2 + n..];
        for command in ["downcase", "upcase"] {
            let pattern = format!(":/{command}}}");
            if rest.starts_with(&pattern) {
                return Some((i + 2 + n + pattern.len(), group, Some(command)));
            }
        }
    }
    None
}

/// Port of `RegexSource.replaceCaptures`.
pub(crate) fn replace_captures(source: &str, line: &str, captures: &[CaptureIndex]) -> String {
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut last = 0;
    while i < source.len() {
        if let Some((end, group, command)) = parse_capture_ref(source, i) {
            if let Some(capture) = captures.get(group) {
                out.push_str(&source[last..i]);
                let mut text = line.get(capture.start..capture.end).unwrap_or("");
                text = text.trim_start_matches('.');
                match command {
                    Some("downcase") => out.push_str(&text.to_lowercase()),
                    Some("upcase") => out.push_str(&text.to_uppercase()),
                    _ => out.push_str(text),
                }
                last = end;
            }
            i = end;
        } else {
            i += source[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&source[last..]);
    out
}

/// A regex source with its anchor variants, ported from `RegExpSource`.
#[derive(Clone, Debug)]
pub(crate) struct RegExpSource {
    pub source: String,
    pub rule_id: RuleId,
    pub has_anchor: bool,
    pub has_back_references: bool,
    anchor_cache: Option<Box<[String; 4]>>,
}

impl RegExpSource {
    pub fn new(source: &str, rule_id: RuleId) -> Self {
        let mut output = String::with_capacity(source.len());
        let mut has_anchor = false;
        let mut last = 0;
        let bytes = source.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() {
            if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
                match bytes[pos + 1] {
                    b'z' => {
                        output.push_str(&source[last..pos]);
                        output.push_str("$(?!\\n)(?<!\\n)");
                        last = pos + 2;
                    }
                    b'A' | b'G' => has_anchor = true,
                    _ => {}
                }
                pos += 1;
            }
            pos += 1;
        }
        let source = if last == 0 {
            source.to_string()
        } else {
            output.push_str(&source[last..]);
            output
        };
        let mut result = Self {
            has_back_references: has_back_references(&source),
            source,
            rule_id,
            has_anchor,
            anchor_cache: None,
        };
        if has_anchor {
            result.anchor_cache = Some(Box::new(build_anchor_cache(&result.source)));
        }
        result
    }

    /// Replaces the source (used for `end`/`while` patterns with back references).
    pub fn with_source(&self, source: &str) -> Self {
        let mut result = Self {
            source: source.to_string(),
            rule_id: self.rule_id,
            has_anchor: self.has_anchor,
            has_back_references: self.has_back_references,
            anchor_cache: None,
        };
        if result.has_anchor {
            result.anchor_cache = Some(Box::new(build_anchor_cache(source)));
        }
        result
    }

    /// Port of `resolveBackReferences`: `\1` becomes the escaped captured text.
    pub fn resolve_back_references(&self, line: &str, captures: &[CaptureIndex]) -> String {
        let bytes = self.source.as_bytes();
        let mut out = String::with_capacity(self.source.len());
        let mut last = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                let n = bytes[i + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                if n > 0 {
                    out.push_str(&self.source[last..i]);
                    let group: usize = self.source[i + 1..i + 1 + n].parse().unwrap_or(usize::MAX);
                    let text = captures
                        .get(group)
                        .and_then(|c| line.get(c.start..c.end))
                        .unwrap_or("");
                    out.push_str(&escape_regexp_characters(text));
                    i += 1 + n;
                    last = i;
                    continue;
                }
            }
            i += 1;
        }
        out.push_str(&self.source[last..]);
        out
    }

    /// The source with `\A` and `\G` disabled according to `allow_a` and `allow_g`.
    pub fn resolve_anchors(&self, allow_a: bool, allow_g: bool) -> &str {
        match &self.anchor_cache {
            Some(cache) => &cache[(allow_a as usize) * 2 + allow_g as usize],
            None => &self.source,
        }
    }
}

fn has_back_references(source: &str) -> bool {
    let bytes = source.as_bytes();
    bytes
        .windows(2)
        .any(|w| w[0] == b'\\' && w[1].is_ascii_digit())
}

/// Builds the `[A0_G0, A0_G1, A1_G0, A1_G1]` variants.
fn build_anchor_cache(source: &str) -> [String; 4] {
    let mut out: [String; 4] = Default::default();
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        for s in out.iter_mut() {
            s.push(ch);
        }
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                chars.next();
                for (i, s) in out.iter_mut().enumerate() {
                    let allow_a = i >= 2;
                    let allow_g = i % 2 == 1;
                    let replaced = match next {
                        'A' if !allow_a => '\u{FFFF}',
                        'G' if !allow_g => '\u{FFFF}',
                        other => other,
                    };
                    s.push(replaced);
                }
            }
        }
    }
    out
}

/// Port of `escapeRegExpCharacters`.
pub(crate) fn escape_regexp_characters(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 2);
    for ch in value.chars() {
        if matches!(
            ch,
            '-' | '\\'
                | '{'
                | '}'
                | '*'
                | '+'
                | '?'
                | '|'
                | '^'
                | '$'
                | '.'
                | ','
                | '['
                | ']'
                | '('
                | ')'
                | '#'
        ) || is_js_whitespace(ch)
        {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The `\s` class of JavaScript regular expressions.
pub(crate) fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(start: usize, end: usize) -> CaptureIndex {
        CaptureIndex {
            start,
            end,
            length: end - start,
        }
    }

    #[test]
    fn replaces_captures_in_names() {
        let caps = [cap(0, 7), cap(0, 3), cap(4, 7)];
        assert_eq!(
            replace_captures("a.$1.${2:/upcase}", "abc def", &caps),
            "a.abc.DEF"
        );
        assert_eq!(replace_captures("keep.$9", "abc", &caps), "keep.$9");
    }

    #[test]
    fn resolves_back_references() {
        let src = RegExpSource::new("^\\1\\)", -1);
        assert!(src.has_back_references);
        assert_eq!(
            src.resolve_back_references("a.b", &[cap(0, 3), cap(0, 3)]),
            "^a\\.b\\)"
        );
    }

    #[test]
    fn anchors() {
        let src = RegExpSource::new("\\G\\Ax\\z", 1);
        assert_eq!(src.source, "\\G\\Ax$(?!\\n)(?<!\\n)");
        assert_eq!(
            src.resolve_anchors(false, false),
            "\\\u{FFFF}\\\u{FFFF}x$(?!\\n)(?<!\\n)"
        );
        assert_eq!(src.resolve_anchors(true, true), "\\G\\Ax$(?!\\n)(?<!\\n)");
    }
}
