//! Scope selectors (`source.js - comment, L:text.html`), ported from `matcher.ts`.

#[derive(Debug, Clone)]
pub(crate) enum Matcher {
    Names(Vec<String>),
    Not(Option<Box<Matcher>>),
    And(Vec<Matcher>),
    Or(Vec<Matcher>),
}

impl Matcher {
    pub fn matches(&self, scopes: &[&str]) -> bool {
        match self {
            Matcher::Names(names) => name_matcher(names, scopes),
            Matcher::Not(inner) => inner.as_ref().is_some_and(|m| !m.matches(scopes)),
            Matcher::And(all) => all.iter().all(|m| m.matches(scopes)),
            Matcher::Or(any) => any.iter().any(|m| m.matches(scopes)),
        }
    }
}

/// A selector alternative with its priority (`L:` is -1, `R:` is 1).
#[derive(Debug, Clone)]
pub(crate) struct MatcherWithPriority {
    pub matcher: Matcher,
    pub priority: i32,
}

/// Parses a selector into its comma separated alternatives.
pub(crate) fn create_matchers(selector: &str) -> Vec<MatcherWithPriority> {
    let mut parser = SelectorParser {
        tokens: tokenize(selector),
        pos: 0,
    };
    let mut results = Vec::new();
    while parser.peek().is_some() {
        let mut priority = 0;
        if let Some(token) = parser.peek() {
            if token.len() == 2 && token.ends_with(':') {
                priority = match token.as_bytes()[0] {
                    b'R' => 1,
                    b'L' => -1,
                    _ => 0,
                };
                parser.next();
            }
        }
        let matcher = parser.parse_conjunction();
        results.push(MatcherWithPriority { matcher, priority });
        if parser.peek() != Some(",") {
            break;
        }
        parser.next();
    }
    results
}

struct SelectorParser<'a> {
    tokens: Vec<&'a str>,
    pos: usize,
}

impl<'a> SelectorParser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.pos).copied()
    }

    fn next(&mut self) {
        self.pos += 1;
    }

    fn parse_operand(&mut self) -> Option<Matcher> {
        let token = self.peek()?;
        if token == "-" {
            self.next();
            let inner = self.parse_operand();
            return Some(Matcher::Not(inner.map(Box::new)));
        }
        if token == "(" {
            self.next();
            let inner = self.parse_inner_expression();
            if self.peek() == Some(")") {
                self.next();
            }
            return Some(inner);
        }
        if is_identifier(token) {
            let mut names = Vec::new();
            while let Some(token) = self.peek().filter(|t| is_identifier(t)) {
                names.push(token.to_string());
                self.next();
            }
            return Some(Matcher::Names(names));
        }
        None
    }

    fn parse_conjunction(&mut self) -> Matcher {
        let mut matchers = Vec::new();
        while let Some(m) = self.parse_operand() {
            matchers.push(m);
        }
        Matcher::And(matchers)
    }

    fn parse_inner_expression(&mut self) -> Matcher {
        let mut matchers = Vec::new();
        loop {
            // `parseConjunction` always returns a matcher in vscode-textmate.
            matchers.push(self.parse_conjunction());
            if matches!(self.peek(), Some("|") | Some(",")) {
                while matches!(self.peek(), Some("|") | Some(",")) {
                    self.next();
                }
            } else {
                break;
            }
        }
        Matcher::Or(matchers)
    }
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_identifier(token: &str) -> bool {
    token.bytes().any(|b| is_word(b) || b == b'.' || b == b':')
}

/// Splits on `([LR]:|[\w\.:][\w\.:\-]*|[\,\|\-\(\)])`, skipping anything else.
fn tokenize(input: &str) -> Vec<&str> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if (b == b'L' || b == b'R') && bytes.get(i + 1) == Some(&b':') {
            tokens.push(&input[i..i + 2]);
            i += 2;
        } else if is_word(b) || b == b'.' || b == b':' {
            let start = i;
            i += 1;
            while i < bytes.len() && (is_word(bytes[i]) || matches!(bytes[i], b'.' | b':' | b'-')) {
                i += 1;
            }
            tokens.push(&input[start..i]);
        } else if matches!(b, b',' | b'|' | b'-' | b'(' | b')') {
            tokens.push(&input[i..i + 1]);
            i += 1;
        } else {
            i += 1;
        }
    }
    tokens
}

fn name_matcher(identifiers: &[String], scopes: &[&str]) -> bool {
    if scopes.len() < identifiers.len() {
        return false;
    }
    let mut last_index = 0;
    identifiers.iter().all(|identifier| {
        for (i, scope) in scopes.iter().enumerate().skip(last_index) {
            if scopes_are_matching(scope, identifier) {
                last_index = i + 1;
                return true;
            }
        }
        false
    })
}

fn scopes_are_matching(this_scope: &str, scope: &str) -> bool {
    if this_scope.is_empty() {
        return false;
    }
    this_scope == scope
        || (this_scope.len() > scope.len()
            && this_scope.starts_with(scope)
            && this_scope.as_bytes()[scope.len()] == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(selector: &str, scopes: &[&str]) -> bool {
        create_matchers(selector)
            .iter()
            .any(|x| x.matcher.matches(scopes))
    }

    #[test]
    fn selectors() {
        assert!(m("source.js", &["source.js", "string.quoted"]));
        assert!(m("source string", &["source.js", "string.quoted"]));
        assert!(!m("string source", &["source.js", "string.quoted"]));
        assert!(!m("source.js - string", &["source.js", "string.quoted"]));
        assert!(m("source.js - comment", &["source.js", "string.quoted"]));
        assert!(m("text, source", &["source.js"]));
        assert!(m("(text | source) - comment", &["source.js"]));
        let ms = create_matchers("L:text.html.markdown - (comment | string)");
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].priority, -1);
    }
}
