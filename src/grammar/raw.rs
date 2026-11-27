//! Raw TextMate grammars, parsed from JSON into an arena of rule descriptions.
//!
//! In vscode-textmate the identity of a rule is the identity of its JSON
//! object. Here each object gets an index in [`RawGrammar::rules`], and that
//! index plays the same role.

use std::collections::HashMap;

use serde_json::{Map, Value};

/// Index of a rule description inside a [`RawGrammar`].
pub(crate) type RawIdx = usize;

/// One rule description (a JSON object of a grammar).
#[derive(Debug, Default)]
pub(crate) struct RawRule {
    pub include: Option<String>,
    pub name: Option<String>,
    pub content_name: Option<String>,
    pub match_: Option<String>,
    pub begin: Option<String>,
    pub end: Option<String>,
    pub while_: Option<String>,
    pub captures: Option<RawCaptures>,
    pub begin_captures: Option<RawCaptures>,
    pub end_captures: Option<RawCaptures>,
    pub while_captures: Option<RawCaptures>,
    pub patterns: Option<Vec<RawIdx>>,
    pub repository: Option<HashMap<String, RawIdx>>,
    pub apply_end_pattern_last: bool,
}

/// Capture rules, by capture number, in ascending order.
pub(crate) type RawCaptures = Vec<(usize, RawIdx)>;

/// A parsed grammar.
#[derive(Debug)]
pub(crate) struct RawGrammar {
    pub scope_name: String,
    pub rules: Vec<RawRule>,
    /// The top level repository of the grammar (without `$self` and `$base`).
    pub repository: HashMap<String, RawIdx>,
    /// The synthetic `$self` rule: the top level patterns, named after the scope.
    pub self_rule: RawIdx,
    /// The grammar object itself used as a rule, as done for injection grammars.
    pub grammar_rule: RawIdx,
    /// `injections` of the grammar: selector and rule.
    pub injections: Vec<(String, RawIdx)>,
    /// `injectionSelector` of the grammar, used when it is injected into others.
    pub injection_selector: Option<String>,
}

impl RawGrammar {
    /// Parses a grammar from a JSON object.
    pub fn from_json(value: &Map<String, Value>) -> Result<Self, String> {
        let scope_name = value
            .get("scopeName")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing `scopeName`".to_string())?
            .to_string();
        let mut parser = Parser { rules: Vec::new() };
        let patterns = parser.patterns(value.get("patterns"));
        let repository = parser
            .repository(value.get("repository"))
            .unwrap_or_default();
        let mut injections = Vec::new();
        if let Some(Value::Object(map)) = value.get("injections") {
            for (selector, rule) in map {
                if let Value::Object(rule) = rule {
                    let idx = parser.rule(rule);
                    injections.push((selector.clone(), idx));
                }
            }
        }
        let self_rule = parser.push(RawRule {
            name: Some(scope_name.clone()),
            patterns: Some(patterns.clone()),
            ..Default::default()
        });
        let grammar_rule = parser.push(RawRule {
            name: non_empty_str(value.get("name")),
            patterns: Some(patterns),
            ..Default::default()
        });
        Ok(Self {
            scope_name,
            rules: parser.rules,
            repository,
            self_rule,
            grammar_rule,
            injections,
            injection_selector: value
                .get("injectionSelector")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

struct Parser {
    rules: Vec<RawRule>,
}

impl Parser {
    fn push(&mut self, rule: RawRule) -> RawIdx {
        self.rules.push(rule);
        self.rules.len() - 1
    }

    fn rule(&mut self, obj: &Map<String, Value>) -> RawIdx {
        let idx = self.push(RawRule::default());
        let mut rule = RawRule {
            include: string(obj.get("include")),
            name: non_empty_str(obj.get("name")),
            content_name: non_empty_str(obj.get("contentName")),
            match_: string(obj.get("match")),
            begin: string(obj.get("begin")),
            end: string(obj.get("end")),
            while_: string(obj.get("while")),
            captures: self.captures(obj.get("captures")),
            begin_captures: self.captures(obj.get("beginCaptures")),
            end_captures: self.captures(obj.get("endCaptures")),
            while_captures: self.captures(obj.get("whileCaptures")),
            patterns: obj.get("patterns").map(|p| self.patterns(Some(p))),
            repository: self.repository(obj.get("repository")),
            apply_end_pattern_last: truthy(obj.get("applyEndPatternLast")),
        };
        // An include-only rule with `include` and no `patterns` behaves like
        // `patterns: [{ include }]`.
        if rule.match_.as_deref().is_none_or(str::is_empty)
            && rule.begin.is_none()
            && rule.patterns.is_none()
        {
            if let Some(include) = rule.include.clone().filter(|s| !s.is_empty()) {
                let child = self.push(RawRule {
                    include: Some(include),
                    ..Default::default()
                });
                rule.patterns = Some(vec![child]);
            }
        }
        self.rules[idx] = rule;
        idx
    }

    fn patterns(&mut self, value: Option<&Value>) -> Vec<RawIdx> {
        match value {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.as_object().map(|obj| self.rule(obj)))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn repository(&mut self, value: Option<&Value>) -> Option<HashMap<String, RawIdx>> {
        let Some(Value::Object(map)) = value else {
            return None;
        };
        let mut repo = HashMap::with_capacity(map.len());
        for (key, rule) in map {
            if let Value::Object(rule) = rule {
                let idx = self.rule(rule);
                repo.insert(key.clone(), idx);
            }
        }
        Some(repo)
    }

    fn captures(&mut self, value: Option<&Value>) -> Option<RawCaptures> {
        let mut out = Vec::new();
        match value {
            Some(Value::Object(map)) => {
                for (key, rule) in map {
                    let Some(number) = parse_int_prefix(key) else {
                        continue;
                    };
                    if let Value::Object(rule) = rule {
                        let idx = self.rule(rule);
                        out.push((number, idx));
                    }
                }
            }
            Some(Value::Array(items)) => {
                for (number, rule) in items.iter().enumerate() {
                    if let Value::Object(rule) = rule {
                        let idx = self.rule(rule);
                        out.push((number, idx));
                    }
                }
            }
            _ => return None,
        }
        out.sort_by_key(|(n, _)| *n);
        Some(out)
    }
}

fn string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn non_empty_str(value: Option<&Value>) -> Option<String> {
    string(value).filter(|s| !s.is_empty())
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `parseInt(key, 10)` for capture keys.
fn parse_int_prefix(key: &str) -> Option<usize> {
    let key = key.trim_start();
    let digits: &str = &key[..key.bytes().take_while(u8::is_ascii_digit).count()];
    digits.parse().ok()
}
