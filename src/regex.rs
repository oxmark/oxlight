//! Oniguruma scanner, the regex engine used by TextMate grammars.
//!
//! This mirrors the `OnigScanner` of `vscode-oniguruma`: a list of patterns is
//! searched from a start position, and the leftmost match wins. When several
//! patterns match at the same position, the first one in the list wins.

use onig::{Regex, RegexOptions, Region, SearchOptions, Syntax};

/// Start and end byte offsets of one capture group.
///
/// Groups that did not participate in the match have `start == end == 0`
/// and `length == 0`, which the tokenizer treats like an empty capture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CaptureIndex {
    pub start: usize,
    pub end: usize,
    pub length: usize,
}

/// A successful scan: the index of the matching pattern and its captures.
#[derive(Debug)]
pub(crate) struct ScanMatch {
    pub index: usize,
    pub captures: Vec<CaptureIndex>,
}

/// A compiled list of patterns.
pub(crate) struct Scanner {
    regexes: Vec<Option<Regex>>,
    /// Whether the pattern uses `\G`, whose result depends on the start position.
    has_g_anchor: Vec<bool>,
}

/// Results of previous searches of a scanner on one string.
///
/// Like vscode-oniguruma, a pattern searched from `from` with a match at `m`
/// gives the same match for any start in `from..=m`, and a pattern without a
/// match from `from` has none from any later start. Patterns using `\G` are
/// not cached.
#[derive(Default)]
pub(crate) struct ScanCache {
    entries: Vec<Option<CachedSearch>>,
}

/// Start position of a search, and its match position and region if any.
type CachedSearch = (usize, Option<(usize, Region)>);

impl Scanner {
    /// Compiles `patterns`. A pattern that Oniguruma rejects never matches.
    // GUESS: vscode-oniguruma throws on an invalid pattern, which makes Shiki
    // fail for the whole document. Skipping the pattern is more forgiving.
    pub fn new<S: AsRef<str>>(patterns: &[S]) -> Self {
        let has_g_anchor = patterns.iter().map(|p| uses_g_anchor(p.as_ref())).collect();
        let regexes = patterns
            .iter()
            .map(|p| {
                Regex::with_options(
                    p.as_ref(),
                    RegexOptions::REGEX_OPTION_CAPTURE_GROUP,
                    Syntax::default(),
                )
                .ok()
            })
            .collect();
        Self {
            regexes,
            has_g_anchor,
        }
    }

    /// Finds the leftmost match of any pattern in `text`, starting at byte `start`.
    #[cfg(test)]
    pub fn find_next_match(&self, text: &str, start: usize) -> Option<ScanMatch> {
        self.find_next_match_cached(text, start, &mut ScanCache::default())
    }

    /// Like `find_next_match`, reusing the results of previous searches on the same `text`.
    pub fn find_next_match_cached(
        &self,
        text: &str,
        start: usize,
        cache: &mut ScanCache,
    ) -> Option<ScanMatch> {
        if cache.entries.len() != self.regexes.len() {
            cache.entries = (0..self.regexes.len()).map(|_| None).collect();
        }
        let mut best: Option<(usize, usize)> = None;
        for (index, regex) in self.regexes.iter().enumerate() {
            let Some(regex) = regex else { continue };
            let cacheable = !self.has_g_anchor[index];
            let reuse = cacheable
                && match &cache.entries[index] {
                    Some((from, None)) => *from <= start,
                    Some((from, Some((pos, _)))) => *from <= start && *pos >= start,
                    None => false,
                };
            if !reuse {
                let mut region = Region::new();
                let found = regex.search_with_options(
                    text,
                    start,
                    text.len(),
                    SearchOptions::SEARCH_OPTION_NONE,
                    Some(&mut region),
                );
                cache.entries[index] = Some((start, found.map(|pos| (pos, region))));
            }
            if let Some((_, Some((pos, _)))) = &cache.entries[index] {
                let pos = *pos;
                if best.is_none_or(|(best_pos, _)| pos < best_pos) {
                    best = Some((pos, index));
                    if pos == start {
                        break;
                    }
                }
            }
        }
        let (_, index) = best?;
        let Some((_, Some((_, region)))) = &cache.entries[index] else {
            return None;
        };
        let captures = (0..region.len())
            .map(|i| match region.pos(i) {
                Some((s, e)) => CaptureIndex {
                    start: s,
                    end: e,
                    length: e - s,
                },
                None => CaptureIndex::default(),
            })
            .collect();
        Some(ScanMatch { index, captures })
    }
}

/// Whether a pattern contains a `\G` escape.
fn uses_g_anchor(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            if bytes[i + 1] == b'G' {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leftmost_then_first_pattern_wins() {
        let scanner = Scanner::new(&["b", "a", "ab"]);
        let m = scanner.find_next_match("xxab", 0).unwrap();
        assert_eq!(m.index, 1);
        assert_eq!(
            m.captures[0],
            CaptureIndex {
                start: 2,
                end: 3,
                length: 1
            }
        );
    }

    #[test]
    fn g_anchor_matches_at_search_start() {
        let scanner = Scanner::new(&["\\Gb"]);
        assert!(scanner.find_next_match("ab", 1).is_some());
        assert!(scanner.find_next_match("ab", 0).is_none());
    }

    #[test]
    fn invalid_pattern_never_matches() {
        let scanner = Scanner::new(&["(", "a"]);
        assert_eq!(scanner.find_next_match("a", 0).unwrap().index, 1);
    }
}
