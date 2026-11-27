//! Compiles raw grammars into rules, ported from `RuleFactory` and
//! `Grammar` of vscode-textmate.

use std::collections::HashMap;
use std::sync::Arc;

use super::matcher::{create_matchers, Matcher};
use super::raw::{RawCaptures, RawGrammar, RawIdx};
use super::rule::{RegExpSource, Rule, RuleId, RuleKind, END_RULE_ID, WHILE_RULE_ID};

/// Looks up raw grammars by scope name.
pub(crate) trait GrammarLookup {
    fn lookup(&self, scope_name: &str) -> Option<Arc<RawGrammar>>;
    /// Scope names of the grammars injected into `scope_name`.
    fn injections(&self, scope_name: &str) -> Vec<String>;
}

/// An injection rule of a compiled grammar.
#[derive(Debug)]
pub(crate) struct Injection {
    pub matcher: Matcher,
    pub rule_id: RuleId,
    pub priority: i32,
}

/// The output of the compiler.
pub(crate) struct CompiledRules {
    pub rules: Vec<Rule>,
    pub root_id: RuleId,
    pub injections: Vec<Injection>,
}

type InstId = usize;
type RuleKey = (InstId, RawIdx);

struct Instance {
    raw: Arc<RawGrammar>,
    base: RuleKey,
}

#[derive(Clone, Copy)]
enum Layer {
    Top(InstId),
    Rule(InstId, RawIdx),
}

/// A repository context: the grammar top level repository, merged with the
/// repositories of the enclosing include-only rules.
#[derive(Clone)]
struct Repo {
    layers: Vec<Layer>,
}

impl Repo {
    fn top(inst: InstId) -> Self {
        Self {
            layers: vec![Layer::Top(inst)],
        }
    }

    fn inst(&self) -> InstId {
        match self.layers[0] {
            Layer::Top(inst) | Layer::Rule(inst, _) => inst,
        }
    }
}

pub(crate) fn compile(root: Arc<RawGrammar>, lookup: &dyn GrammarLookup) -> CompiledRules {
    let self_rule = root.self_rule;
    let root_scope = root.scope_name.clone();
    let mut c = Compiler {
        lookup,
        instances: vec![Instance {
            raw: root,
            base: (0, self_rule),
        }],
        included: HashMap::new(),
        memo: HashMap::new(),
        rules: vec![None],
    };
    let root_id = c.rule_id((0, self_rule), &Repo::top(0));

    let mut injections = Vec::new();
    let root_injections: Vec<(String, RawIdx)> = c.instances[0].raw.injections.clone();
    for (selector, idx) in root_injections {
        let rule_id = c.rule_id((0, idx), &Repo::top(0));
        for m in create_matchers(&selector) {
            injections.push(Injection {
                matcher: m.matcher,
                rule_id,
                priority: m.priority,
            });
        }
    }
    for scope in lookup.injections(&root_scope) {
        let Some(inst) = c.external_grammar(&scope, None) else {
            continue;
        };
        let raw = c.instances[inst].raw.clone();
        let Some(selector) = &raw.injection_selector else {
            continue;
        };
        let rule_id = c.rule_id((inst, raw.grammar_rule), &Repo::top(inst));
        for m in create_matchers(selector) {
            injections.push(Injection {
                matcher: m.matcher,
                rule_id,
                priority: m.priority,
            });
        }
    }
    injections.sort_by_key(|i| i.priority);

    let rules = c
        .rules
        .into_iter()
        .map(|r| {
            r.unwrap_or_else(|| Rule::new(None, None, RuleKind::Capture { retokenize_with: 0 }))
        })
        .collect();
    CompiledRules {
        rules,
        root_id,
        injections,
    }
}

struct Compiler<'a> {
    lookup: &'a dyn GrammarLookup,
    instances: Vec<Instance>,
    included: HashMap<String, InstId>,
    memo: HashMap<RuleKey, RuleId>,
    rules: Vec<Option<Rule>>,
}

impl Compiler<'_> {
    fn register(&mut self) -> RuleId {
        self.rules.push(None);
        (self.rules.len() - 1) as RuleId
    }

    fn lookup_repo(&self, repo: &Repo, name: &str) -> Option<RuleKey> {
        let inst = repo.inst();
        match name {
            "$self" => return Some((inst, self.instances[inst].raw.self_rule)),
            "$base" => return Some(self.instances[inst].base),
            _ => {}
        }
        for layer in repo.layers.iter().rev() {
            let found = match *layer {
                Layer::Top(i) => self.instances[i]
                    .raw
                    .repository
                    .get(name)
                    .map(|&idx| (i, idx)),
                Layer::Rule(i, idx) => self.instances[i].raw.rules[idx]
                    .repository
                    .as_ref()
                    .and_then(|r| r.get(name))
                    .map(|&idx| (i, idx)),
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    fn external_grammar(&mut self, scope_name: &str, repo: Option<&Repo>) -> Option<InstId> {
        if let Some(&inst) = self.included.get(scope_name) {
            return Some(inst);
        }
        let raw = self.lookup.lookup(scope_name)?;
        let inst = self.instances.len();
        let base = match repo {
            Some(repo) => self
                .lookup_repo(repo, "$base")
                .unwrap_or((inst, raw.self_rule)),
            None => (inst, raw.self_rule),
        };
        self.instances.push(Instance { raw, base });
        self.included.insert(scope_name.to_string(), inst);
        Some(inst)
    }

    /// Port of `RuleFactory.getCompiledRuleId`.
    fn rule_id(&mut self, key: RuleKey, repo: &Repo) -> RuleId {
        if let Some(&id) = self.memo.get(&key) {
            return id;
        }
        let id = self.register();
        self.memo.insert(key, id);
        let raw = self.instances[key.0].raw.clone();
        let desc = &raw.rules[key.1];
        let inst = key.0;

        let rule = if let Some(m) = desc.match_.as_deref().filter(|m| !m.is_empty()) {
            let captures = self.captures(inst, desc.captures.as_ref(), repo);
            Rule::new(
                desc.name.clone(),
                None,
                RuleKind::Match {
                    regex: RegExpSource::new(m, id),
                    captures,
                },
            )
        } else if desc.begin.is_none() {
            let repo = if desc.repository.is_some() {
                let mut merged = repo.clone();
                merged.layers.push(Layer::Rule(inst, key.1));
                merged
            } else {
                repo.clone()
            };
            let patterns = desc.patterns.clone().unwrap_or_default();
            let (patterns, has_missing_patterns) = self.patterns(inst, &patterns, &repo);
            Rule::new(
                desc.name.clone(),
                desc.content_name.clone(),
                RuleKind::IncludeOnly {
                    patterns,
                    has_missing_patterns,
                },
            )
        } else if let Some(while_) = desc.while_.as_deref().filter(|w| !w.is_empty()) {
            let begin_captures = self.captures(
                inst,
                desc.begin_captures.as_ref().or(desc.captures.as_ref()),
                repo,
            );
            let while_captures = self.captures(
                inst,
                desc.while_captures.as_ref().or(desc.captures.as_ref()),
                repo,
            );
            let patterns = desc.patterns.clone().unwrap_or_default();
            let (patterns, has_missing_patterns) = self.patterns(inst, &patterns, repo);
            Rule::new(
                desc.name.clone(),
                desc.content_name.clone(),
                RuleKind::BeginWhile {
                    begin: RegExpSource::new(desc.begin.as_deref().unwrap_or(""), id),
                    begin_captures,
                    while_: RegExpSource::new(while_, WHILE_RULE_ID),
                    while_captures,
                    patterns,
                    has_missing_patterns,
                },
            )
        } else {
            let begin_captures = self.captures(
                inst,
                desc.begin_captures.as_ref().or(desc.captures.as_ref()),
                repo,
            );
            let end_captures = self.captures(
                inst,
                desc.end_captures.as_ref().or(desc.captures.as_ref()),
                repo,
            );
            let patterns = desc.patterns.clone().unwrap_or_default();
            let (patterns, has_missing_patterns) = self.patterns(inst, &patterns, repo);
            let end = desc
                .end
                .as_deref()
                .filter(|e| !e.is_empty())
                .unwrap_or("\u{FFFF}");
            Rule::new(
                desc.name.clone(),
                desc.content_name.clone(),
                RuleKind::BeginEnd {
                    begin: RegExpSource::new(desc.begin.as_deref().unwrap_or(""), id),
                    begin_captures,
                    end: RegExpSource::new(end, END_RULE_ID),
                    end_captures,
                    apply_end_pattern_last: desc.apply_end_pattern_last,
                    patterns,
                    has_missing_patterns,
                },
            )
        };
        self.rules[id as usize] = Some(rule);
        id
    }

    /// Port of `RuleFactory._compileCaptures`.
    fn captures(
        &mut self,
        inst: InstId,
        captures: Option<&RawCaptures>,
        repo: &Repo,
    ) -> Vec<Option<RuleId>> {
        let Some(captures) = captures else {
            return Vec::new();
        };
        let max = captures.iter().map(|(n, _)| *n).max().unwrap_or(0);
        if captures.is_empty() {
            return Vec::new();
        }
        let mut out = vec![None; max + 1];
        let raw = self.instances[inst].raw.clone();
        for &(number, idx) in captures {
            let desc = &raw.rules[idx];
            let retokenize_with = if desc.patterns.is_some() {
                self.rule_id((inst, idx), repo)
            } else {
                0
            };
            let id = self.register();
            self.rules[id as usize] = Some(Rule::new(
                desc.name.clone(),
                desc.content_name.clone(),
                RuleKind::Capture { retokenize_with },
            ));
            out[number] = Some(id);
        }
        out
    }

    /// Port of `RuleFactory._compilePatterns`.
    fn patterns(&mut self, inst: InstId, patterns: &[RawIdx], repo: &Repo) -> (Vec<RuleId>, bool) {
        let raw = self.instances[inst].raw.clone();
        let mut out = Vec::new();
        for &idx in patterns {
            let pattern = &raw.rules[idx];
            let rule_id = match pattern.include.as_deref().filter(|i| !i.is_empty()) {
                Some(include) => self.include(include, repo),
                None => Some(self.rule_id((inst, idx), repo)),
            };
            let Some(rule_id) = rule_id else { continue };
            if let Some(rule) = &self.rules[rule_id as usize] {
                if let Some((patterns, missing)) = rule.patterns() {
                    if missing && patterns.is_empty() {
                        continue;
                    }
                }
            }
            out.push(rule_id);
        }
        let missing = patterns.len() != out.len();
        (out, missing)
    }

    fn include(&mut self, include: &str, repo: &Repo) -> Option<RuleId> {
        if include == "$base" || include == "$self" {
            let key = self.lookup_repo(repo, include)?;
            return Some(self.rule_id(key, repo));
        }
        match include.find('#') {
            Some(0) => {
                let key = self.lookup_repo(repo, &include[1..])?;
                Some(self.rule_id(key, repo))
            }
            found => {
                let (scope, name) = match found {
                    Some(i) => (&include[..i], Some(&include[i + 1..])),
                    None => (include, None),
                };
                let inst = self.external_grammar(scope, Some(repo))?;
                let raw = self.instances[inst].raw.clone();
                let key = match name {
                    Some(name) => (inst, *raw.repository.get(name)?),
                    None => (inst, raw.self_rule),
                };
                Some(self.rule_id(key, &Repo::top(inst)))
            }
        }
    }
}
