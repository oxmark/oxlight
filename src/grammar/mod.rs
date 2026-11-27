//! TextMate grammar engine, ported from vscode-textmate.

mod compile;
mod matcher;
pub(crate) mod raw;
mod rule;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(crate) use compile::GrammarLookup;
use compile::Injection;
use raw::RawGrammar;
pub(crate) use rule::is_js_whitespace;
use rule::{RegExpSource, Rule, RuleId, RuleKind, END_RULE_ID, WHILE_RULE_ID};

use std::cell::RefCell;

use crate::regex::{CaptureIndex, ScanCache, Scanner};
use crate::theme::{ScopeStack, StyleAttributes, TextMateTheme, FONT_STYLE_NOT_SET};

// ---------------------------------------------------------------------------
// Token attributes
// ---------------------------------------------------------------------------

/// `OptionalStandardTokenType.NotSet`.
const TOKEN_TYPE_NOT_SET: u8 = 8;

/// The attributes vscode-textmate encodes in 32 bits, kept unpacked.
///
/// Two adjacent tokens with equal attributes are merged, exactly like the
/// binary tokens of `tokenizeLine2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct TokenAttributes {
    pub language_id: u32,
    pub token_type: u8,
    pub font_style: u8,
    pub foreground: u32,
    pub background: u32,
}

impl TokenAttributes {
    fn set(
        mut self,
        language_id: u32,
        token_type: u8,
        font_style: i8,
        foreground: u32,
        background: u32,
    ) -> Self {
        if language_id != 0 {
            self.language_id = language_id;
        }
        if token_type != TOKEN_TYPE_NOT_SET {
            self.token_type = token_type;
        }
        if font_style != FONT_STYLE_NOT_SET {
            self.font_style = font_style as u8;
        }
        if foreground != 0 {
            self.foreground = foreground;
        }
        if background != 0 {
            self.background = background;
        }
        self
    }

    fn merge(self, token_type: u8, style: Option<StyleAttributes>) -> Self {
        let (font_style, fg, bg) = match style {
            Some(s) => (s.font_style, s.foreground_id, s.background_id),
            None => (FONT_STYLE_NOT_SET, 0, 0),
        };
        self.set(0, token_type, font_style, fg, bg)
    }
}

/// Port of `_toStandardTokenType`: `\b(comment|string|regex|meta\.embedded)\b`.
fn standard_token_type(scope: &str) -> u8 {
    let bytes = scope.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    for i in 0..bytes.len() {
        if i > 0 && is_word(bytes[i - 1]) {
            continue;
        }
        for (word, kind) in [
            ("comment", 1),
            ("string", 2),
            ("regex", 3),
            ("meta.embedded", 0),
        ] {
            if scope[i..].starts_with(word) {
                let end = i + word.len();
                if end == bytes.len() || !is_word(bytes[end]) {
                    return kind;
                }
            }
        }
    }
    TOKEN_TYPE_NOT_SET
}

// ---------------------------------------------------------------------------
// Scope stacks and the state stack
// ---------------------------------------------------------------------------

/// Port of `AttributedScopeStack`.
#[derive(Debug)]
pub(crate) struct AttributedScopeStack {
    scope_path: Arc<ScopeStack>,
    attributes: TokenAttributes,
}

impl AttributedScopeStack {
    fn create_root(
        scope_name: &str,
        attributes: TokenAttributes,
        theme: &TextMateTheme,
    ) -> Arc<Self> {
        let scope_path = Arc::new(ScopeStack {
            parent: None,
            scope_name: scope_name.to_string(),
        });
        let style = theme.match_path(&scope_path);
        let attributes = attributes.merge(standard_token_type(scope_name), style);
        Arc::new(Self {
            scope_path,
            attributes,
        })
    }

    fn push_attributed(
        self: &Arc<Self>,
        scope_path: Option<&str>,
        theme: &TextMateTheme,
    ) -> Arc<Self> {
        let Some(scope_path) = scope_path else {
            return self.clone();
        };
        let mut result = self.clone();
        for scope in scope_path.split(' ') {
            let new_path = result.scope_path.push(scope);
            let style = theme.match_path(&new_path);
            let attributes = result.attributes.merge(standard_token_type(scope), style);
            result = Arc::new(Self {
                scope_path: new_path,
                attributes,
            });
        }
        result
    }

    /// The scope names, outermost first.
    pub fn scope_names(&self) -> Vec<&str> {
        self.scope_path.segments()
    }
}

static LINE_IDS: AtomicU64 = AtomicU64::new(1);

/// The tokenizer state at the end of a line, needed to tokenize the next one.
///
/// Port of `StateStackImpl`. `enter_pos` and `anchor_pos` are only meaningful
/// on the line where the frame was pushed (vscode-textmate resets them before
/// each line); `line_id` records that line.
#[derive(Debug)]
pub(crate) struct StateStack {
    parent: Option<Arc<StateStack>>,
    rule_id: RuleId,
    enter_pos: i64,
    anchor_pos: i64,
    line_id: u64,
    begin_rule_captured_eol: bool,
    end_rule: Option<Arc<str>>,
    name_scopes_list: Arc<AttributedScopeStack>,
    content_name_scopes_list: Arc<AttributedScopeStack>,
    depth: usize,
}

impl StateStack {
    fn enter_pos(&self, line_id: u64) -> i64 {
        if self.line_id == line_id {
            self.enter_pos
        } else {
            -1
        }
    }

    fn anchor_pos(&self, line_id: u64) -> i64 {
        if self.line_id == line_id {
            self.anchor_pos
        } else {
            -1
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        self: &Arc<Self>,
        rule_id: RuleId,
        enter_pos: i64,
        anchor_pos: i64,
        line_id: u64,
        begin_rule_captured_eol: bool,
        end_rule: Option<Arc<str>>,
        name_scopes_list: Arc<AttributedScopeStack>,
        content_name_scopes_list: Arc<AttributedScopeStack>,
    ) -> Arc<Self> {
        Arc::new(Self {
            parent: Some(self.clone()),
            rule_id,
            enter_pos,
            anchor_pos,
            line_id,
            begin_rule_captured_eol,
            end_rule,
            name_scopes_list,
            content_name_scopes_list,
            depth: self.depth + 1,
        })
    }

    fn pop(&self) -> Option<Arc<Self>> {
        self.parent.clone()
    }

    fn safe_pop(self: &Arc<Self>) -> Arc<Self> {
        self.parent.clone().unwrap_or_else(|| self.clone())
    }

    fn with_content_name_scopes_list(
        self: &Arc<Self>,
        list: Arc<AttributedScopeStack>,
    ) -> Arc<Self> {
        if Arc::ptr_eq(&self.content_name_scopes_list, &list) {
            return self.clone();
        }
        Arc::new(Self {
            parent: self.parent.clone(),
            rule_id: self.rule_id,
            enter_pos: self.enter_pos,
            anchor_pos: self.anchor_pos,
            line_id: self.line_id,
            begin_rule_captured_eol: self.begin_rule_captured_eol,
            end_rule: self.end_rule.clone(),
            name_scopes_list: self.name_scopes_list.clone(),
            content_name_scopes_list: list,
            depth: self.depth,
        })
    }

    fn with_end_rule(self: &Arc<Self>, end_rule: String) -> Arc<Self> {
        if self.end_rule.as_deref() == Some(end_rule.as_str()) {
            return self.clone();
        }
        Arc::new(Self {
            parent: self.parent.clone(),
            rule_id: self.rule_id,
            enter_pos: self.enter_pos,
            anchor_pos: self.anchor_pos,
            line_id: self.line_id,
            begin_rule_captured_eol: self.begin_rule_captured_eol,
            end_rule: Some(end_rule.into()),
            name_scopes_list: self.name_scopes_list.clone(),
            content_name_scopes_list: self.content_name_scopes_list.clone(),
            depth: self.depth,
        })
    }

    fn has_same_rule_as(&self, other: &StateStack, line_id: u64) -> bool {
        let other_enter = other.enter_pos(line_id);
        let mut el = Some(self);
        while let Some(e) = el {
            if e.enter_pos(line_id) != other_enter {
                break;
            }
            if e.rule_id == other.rule_id {
                return true;
            }
            el = e.parent.as_deref();
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Line tokens
// ---------------------------------------------------------------------------

/// One token of a tokenized line: start byte offset and attributes.
#[derive(Clone, Debug)]
pub(crate) struct RawToken {
    pub start: usize,
    pub attributes: TokenAttributes,
    /// Scope names, filled only when scopes were requested.
    pub scopes: Option<Vec<String>>,
}

struct LineTokens {
    tokens: Vec<RawToken>,
    last_end: i64,
    with_scopes: bool,
}

impl LineTokens {
    fn produce(&mut self, stack: &StateStack, end: usize) {
        self.produce_from_scopes(&stack.content_name_scopes_list, end);
    }

    fn produce_from_scopes(&mut self, scopes: &AttributedScopeStack, end: usize) {
        let end = end as i64;
        if self.last_end >= end {
            return;
        }
        let attributes = scopes.attributes;
        if !self.with_scopes {
            if let Some(last) = self.tokens.last() {
                if last.attributes == attributes {
                    self.last_end = end;
                    return;
                }
            }
        }
        self.tokens.push(RawToken {
            start: self.last_end.max(0) as usize,
            attributes,
            scopes: self.with_scopes.then(|| {
                scopes
                    .scope_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            }),
        });
        self.last_end = end;
    }

    fn finish(mut self, stack: &StateStack, line_length: usize) -> Vec<RawToken> {
        if self
            .tokens
            .last()
            .is_some_and(|t| t.start == line_length - 1)
        {
            self.tokens.pop();
        }
        if self.tokens.is_empty() {
            self.last_end = -1;
            self.produce(stack, line_length);
            if let Some(t) = self.tokens.last_mut() {
                t.start = 0;
            }
        }
        self.tokens
    }
}

// ---------------------------------------------------------------------------
// Compiled grammar
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ListKind {
    Patterns,
    While,
}

#[derive(PartialEq, Eq, Hash)]
struct ScannerKey {
    rule_id: RuleId,
    kind: ListKind,
    end: Option<Arc<str>>,
}

/// The pattern list of a rule and its compiled scanners, one per anchor variant.
struct ScannerSet {
    list: Vec<RegExpSource>,
    has_anchors: bool,
    slots: [Option<Arc<CompiledScanner>>; 4],
}

struct CompiledScanner {
    scanner: Scanner,
    rule_ids: Vec<RuleId>,
}

/// A grammar compiled for one root scope, with all the grammars it includes.
pub(crate) struct Grammar {
    rules: Vec<Rule>,
    root_id: RuleId,
    injections: Vec<Injection>,
    scanners: Mutex<HashMap<ScannerKey, ScannerSet>>,
}

/// Result of tokenizing one line.
pub(crate) struct LineResult {
    pub tokens: Vec<RawToken>,
    pub state: Arc<StateStack>,
    /// Whether the time limit stopped the tokenization of the line.
    #[allow(dead_code)]
    pub stopped_early: bool,
}

impl Grammar {
    pub fn new(root: Arc<RawGrammar>, lookup: &dyn GrammarLookup) -> Self {
        let compiled = compile::compile(root, lookup);
        Self {
            rules: compiled.rules,
            root_id: compiled.root_id,
            injections: compiled.injections,
            scanners: Mutex::new(HashMap::new()),
        }
    }

    fn rule(&self, id: RuleId) -> &Rule {
        &self.rules[id as usize]
    }

    /// Port of `collectPatterns`, guarded against include cycles.
    fn collect_patterns(&self, id: RuleId, out: &mut Vec<RegExpSource>, path: &mut Vec<RuleId>) {
        match &self.rule(id).kind {
            RuleKind::Match { regex, .. } => out.push(regex.clone()),
            RuleKind::BeginEnd { begin, .. } | RuleKind::BeginWhile { begin, .. } => {
                out.push(begin.clone())
            }
            RuleKind::IncludeOnly { patterns, .. } => {
                if path.contains(&id) {
                    return;
                }
                path.push(id);
                for &p in patterns {
                    self.collect_patterns(p, out, path);
                }
                path.pop();
            }
            RuleKind::Capture { .. } => {}
        }
    }

    fn build_list(&self, id: RuleId, kind: ListKind, end: Option<&str>) -> Vec<RegExpSource> {
        let rule = self.rule(id);
        let mut list = Vec::new();
        match kind {
            ListKind::While => {
                if let RuleKind::BeginWhile { while_, .. } = &rule.kind {
                    if while_.has_back_references {
                        list.push(while_.with_source(end.unwrap_or("\u{FFFF}")));
                    } else {
                        list.push(while_.clone());
                    }
                }
            }
            ListKind::Patterns => {
                if let Some((patterns, _)) = rule.patterns() {
                    let mut path = vec![id];
                    for &p in patterns {
                        self.collect_patterns(p, &mut list, &mut path);
                    }
                }
                if let RuleKind::BeginEnd {
                    end: end_src,
                    apply_end_pattern_last,
                    ..
                } = &rule.kind
                {
                    let end_src = match end {
                        Some(e) if end_src.has_back_references => end_src.with_source(e),
                        _ => end_src.clone(),
                    };
                    if *apply_end_pattern_last {
                        list.push(end_src);
                    } else {
                        list.insert(0, end_src);
                    }
                }
            }
        }
        list
    }

    fn scanner(
        &self,
        id: RuleId,
        kind: ListKind,
        end: Option<&Arc<str>>,
        allow_a: bool,
        allow_g: bool,
    ) -> Arc<CompiledScanner> {
        let uses_end = match (&self.rule(id).kind, kind) {
            (RuleKind::BeginEnd { end, .. }, ListKind::Patterns) => end.has_back_references,
            (RuleKind::BeginWhile { while_, .. }, ListKind::While) => while_.has_back_references,
            _ => false,
        };
        let end = if uses_end { end.cloned() } else { None };
        let key = ScannerKey {
            rule_id: id,
            kind,
            end,
        };
        let mut scanners = self.scanners.lock().unwrap_or_else(|e| e.into_inner());
        let set = scanners.entry(key).or_insert_with_key(|key| {
            let list = self.build_list(id, kind, key.end.as_deref());
            ScannerSet {
                has_anchors: list.iter().any(|s| s.has_anchor),
                list,
                slots: Default::default(),
            }
        });
        let slot = if set.has_anchors {
            (allow_a as usize) * 2 + allow_g as usize
        } else {
            0
        };
        if let Some(compiled) = &set.slots[slot] {
            return compiled.clone();
        }
        let sources: Vec<&str> = set
            .list
            .iter()
            .map(|s| s.resolve_anchors(allow_a, allow_g))
            .collect();
        let compiled = Arc::new(CompiledScanner {
            scanner: Scanner::new(&sources),
            rule_ids: set.list.iter().map(|s| s.rule_id).collect(),
        });
        set.slots[slot] = Some(compiled.clone());
        compiled
    }

    /// Tokenizes one line (without its line ending).
    pub fn tokenize_line(
        &self,
        theme: &TextMateTheme,
        line: &str,
        prev: Option<&Arc<StateStack>>,
        time_limit: Option<std::time::Duration>,
        with_scopes: bool,
    ) -> LineResult {
        let line_id = LINE_IDS.fetch_add(1, Ordering::Relaxed);
        let (is_first_line, state) = match prev {
            Some(prev) => (false, prev.clone()),
            None => {
                let defaults = theme.defaults();
                let default_attributes = TokenAttributes::default().set(
                    1,
                    TOKEN_TYPE_NOT_SET,
                    defaults.font_style,
                    defaults.foreground_id,
                    defaults.background_id,
                );
                let root_scope = self.rule(self.root_id).get_name(None, None);
                let scopes = match &root_scope {
                    Some(name) => {
                        AttributedScopeStack::create_root(name, default_attributes, theme)
                    }
                    None => Arc::new(AttributedScopeStack {
                        scope_path: Arc::new(ScopeStack {
                            parent: None,
                            scope_name: "unknown".into(),
                        }),
                        attributes: default_attributes,
                    }),
                };
                let state = Arc::new(StateStack {
                    parent: None,
                    rule_id: self.root_id,
                    enter_pos: -1,
                    anchor_pos: -1,
                    line_id: 0,
                    begin_rule_captured_eol: false,
                    end_rule: None,
                    name_scopes_list: scopes.clone(),
                    content_name_scopes_list: scopes,
                    depth: 1,
                });
                (true, state)
            }
        };
        let mut text = String::with_capacity(line.len() + 1);
        text.push_str(line);
        text.push('\n');
        let mut tokens = LineTokens {
            tokens: Vec::new(),
            last_end: 0,
            with_scopes,
        };
        let ctx = Ctx {
            grammar: self,
            theme,
            line_id,
            deadline: time_limit.map(|d| Instant::now() + d),
            caches: RefCell::new(HashMap::new()),
        };
        let (state, stopped_early) =
            ctx.tokenize_string(&text, is_first_line, 0, state, &mut tokens, true, true);
        LineResult {
            tokens: tokens.finish(&state, text.len()),
            state,
            stopped_early,
        }
    }
}

struct Ctx<'a> {
    grammar: &'a Grammar,
    theme: &'a TextMateTheme,
    line_id: u64,
    deadline: Option<Instant>,
    /// Search caches, by scanner and length of the searched text (the
    /// searched texts of one line are all prefixes of that line).
    caches: RefCell<HashMap<(usize, usize), ScanCache>>,
}

impl Ctx<'_> {
    fn find(
        &self,
        scanner: &Arc<CompiledScanner>,
        text: &str,
        start: usize,
    ) -> Option<crate::regex::ScanMatch> {
        let key = (Arc::as_ptr(scanner) as usize, text.len());
        let mut caches = self.caches.borrow_mut();
        let cache = caches.entry(key).or_default();
        scanner.scanner.find_next_match_cached(text, start, cache)
    }
}

struct MatchResult {
    captures: Vec<CaptureIndex>,
    rule_id: RuleId,
}

impl Ctx<'_> {
    /// Port of `_tokenizeString`. Returns the final stack and whether it stopped early.
    #[allow(clippy::too_many_arguments)]
    fn tokenize_string(
        &self,
        line: &str,
        mut is_first_line: bool,
        mut line_pos: usize,
        mut stack: Arc<StateStack>,
        tokens: &mut LineTokens,
        check_while_conditions: bool,
        use_deadline: bool,
    ) -> (Arc<StateStack>, bool) {
        let line_length = line.len();
        let mut anchor_position: i64 = -1;
        if check_while_conditions {
            let r = self.check_while_conditions(line, is_first_line, line_pos, stack, tokens);
            stack = r.0;
            line_pos = r.1;
            anchor_position = r.2;
            is_first_line = r.3;
        }
        loop {
            if use_deadline {
                if let Some(deadline) = self.deadline {
                    if Instant::now() > deadline {
                        return (stack, true);
                    }
                }
            }
            let Some(r) = self.match_rule_or_injections(
                line,
                is_first_line,
                line_pos,
                &stack,
                anchor_position,
            ) else {
                tokens.produce(&stack, line_length);
                return (stack, false);
            };
            let captures = r.captures;
            let has_advanced = captures.first().is_some_and(|c| c.end > line_pos);
            let c0 = captures[0];

            if r.rule_id == END_RULE_ID {
                let popped_rule = self.grammar.rule(stack.rule_id);
                tokens.produce(&stack, c0.start);
                stack = stack.with_content_name_scopes_list(stack.name_scopes_list.clone());
                if let RuleKind::BeginEnd { end_captures, .. } = &popped_rule.kind {
                    self.handle_captures(
                        line,
                        is_first_line,
                        &stack,
                        tokens,
                        end_captures,
                        &captures,
                    );
                }
                tokens.produce(&stack, c0.end);
                let popped = stack.clone();
                let Some(parent) = stack.pop() else {
                    // Popping the root should never happen; stop safely.
                    tokens.produce(&stack, line_length);
                    return (stack, false);
                };
                stack = parent;
                anchor_position = popped.anchor_pos(self.line_id);
                if !has_advanced && popped.enter_pos(self.line_id) == line_pos as i64 {
                    // Grammar pushed & popped a rule without advancing.
                    stack = popped;
                    tokens.produce(&stack, line_length);
                    return (stack, false);
                }
            } else {
                let rule = self.grammar.rule(r.rule_id);
                tokens.produce(&stack, c0.start);
                let before_push = stack.clone();
                let scope_name = rule.get_name(Some(line), Some(&captures));
                let name_scopes_list = stack
                    .content_name_scopes_list
                    .push_attributed(scope_name.as_deref(), self.theme);
                stack = stack.push(
                    r.rule_id,
                    line_pos as i64,
                    anchor_position,
                    self.line_id,
                    c0.end == line_length,
                    None,
                    name_scopes_list.clone(),
                    name_scopes_list.clone(),
                );
                match &rule.kind {
                    RuleKind::BeginEnd {
                        begin_captures,
                        end,
                        ..
                    } => {
                        self.handle_captures(
                            line,
                            is_first_line,
                            &stack,
                            tokens,
                            begin_captures,
                            &captures,
                        );
                        tokens.produce(&stack, c0.end);
                        anchor_position = c0.end as i64;
                        let content_name = rule.get_content_name(line, &captures);
                        let content_list =
                            name_scopes_list.push_attributed(content_name.as_deref(), self.theme);
                        stack = stack.with_content_name_scopes_list(content_list);
                        if end.has_back_references {
                            stack =
                                stack.with_end_rule(end.resolve_back_references(line, &captures));
                        }
                        if !has_advanced && before_push.has_same_rule_as(&stack, self.line_id) {
                            // Grammar pushed the same rule without advancing.
                            stack = stack.pop().unwrap_or(stack);
                            tokens.produce(&stack, line_length);
                            return (stack, false);
                        }
                    }
                    RuleKind::BeginWhile {
                        begin_captures,
                        while_,
                        ..
                    } => {
                        self.handle_captures(
                            line,
                            is_first_line,
                            &stack,
                            tokens,
                            begin_captures,
                            &captures,
                        );
                        tokens.produce(&stack, c0.end);
                        anchor_position = c0.end as i64;
                        let content_name = rule.get_content_name(line, &captures);
                        let content_list =
                            name_scopes_list.push_attributed(content_name.as_deref(), self.theme);
                        stack = stack.with_content_name_scopes_list(content_list);
                        if while_.has_back_references {
                            stack = stack
                                .with_end_rule(while_.resolve_back_references(line, &captures));
                        }
                        if !has_advanced && before_push.has_same_rule_as(&stack, self.line_id) {
                            stack = stack.pop().unwrap_or(stack);
                            tokens.produce(&stack, line_length);
                            return (stack, false);
                        }
                    }
                    _ => {
                        let empty = Vec::new();
                        let rule_captures = match &rule.kind {
                            RuleKind::Match { captures, .. } => captures,
                            _ => &empty,
                        };
                        self.handle_captures(
                            line,
                            is_first_line,
                            &stack,
                            tokens,
                            rule_captures,
                            &captures,
                        );
                        tokens.produce(&stack, c0.end);
                        stack = stack.pop().unwrap_or(stack);
                        if !has_advanced {
                            // Grammar is not advancing, nor is it pushing/popping.
                            stack = stack.safe_pop();
                            tokens.produce(&stack, line_length);
                            return (stack, false);
                        }
                    }
                }
            }
            if c0.end > line_pos {
                line_pos = c0.end;
                is_first_line = false;
            }
        }
    }

    /// Port of `_checkWhileConditions`.
    fn check_while_conditions(
        &self,
        line: &str,
        mut is_first_line: bool,
        mut line_pos: usize,
        mut stack: Arc<StateStack>,
        tokens: &mut LineTokens,
    ) -> (Arc<StateStack>, usize, i64, bool) {
        let mut anchor_position: i64 = if stack.begin_rule_captured_eol { 0 } else { -1 };
        let mut while_rules: Vec<Arc<StateStack>> = Vec::new();
        let mut node = Some(stack.clone());
        while let Some(n) = node {
            if matches!(
                self.grammar.rule(n.rule_id).kind,
                RuleKind::BeginWhile { .. }
            ) {
                while_rules.push(n.clone());
            }
            node = n.pop();
        }
        while let Some(frame) = while_rules.pop() {
            let scanner = self.grammar.scanner(
                frame.rule_id,
                ListKind::While,
                frame.end_rule.as_ref(),
                is_first_line,
                line_pos as i64 == anchor_position,
            );
            let found = self.find(&scanner, line, line_pos);
            match found {
                Some(m) => {
                    if scanner.rule_ids[m.index] != WHILE_RULE_ID {
                        stack = frame.pop().unwrap_or(frame);
                        break;
                    }
                    if let Some(&c0) = m.captures.first() {
                        tokens.produce(&frame, c0.start);
                        if let RuleKind::BeginWhile { while_captures, .. } =
                            &self.grammar.rule(frame.rule_id).kind
                        {
                            self.handle_captures(
                                line,
                                is_first_line,
                                &frame,
                                tokens,
                                while_captures,
                                &m.captures,
                            );
                        }
                        tokens.produce(&frame, c0.end);
                        anchor_position = c0.end as i64;
                        if c0.end > line_pos {
                            line_pos = c0.end;
                            is_first_line = false;
                        }
                    }
                }
                None => {
                    stack = frame.pop().unwrap_or(frame);
                    break;
                }
            }
        }
        (stack, line_pos, anchor_position, is_first_line)
    }

    fn match_rule_or_injections(
        &self,
        line: &str,
        is_first_line: bool,
        line_pos: usize,
        stack: &Arc<StateStack>,
        anchor_position: i64,
    ) -> Option<MatchResult> {
        let allow_g = line_pos as i64 == anchor_position;
        let scanner = self.grammar.scanner(
            stack.rule_id,
            ListKind::Patterns,
            stack.end_rule.as_ref(),
            is_first_line,
            allow_g,
        );
        let match_result = self.find(&scanner, line, line_pos).map(|m| MatchResult {
            rule_id: scanner.rule_ids[m.index],
            captures: m.captures,
        });
        if self.grammar.injections.is_empty() {
            return match_result;
        }
        let Some((injection, priority_match)) =
            self.match_injections(line, is_first_line, line_pos, stack, allow_g)
        else {
            return match_result;
        };
        let Some(match_result) = match_result else {
            return Some(injection);
        };
        let match_score = match_result.captures[0].start;
        let injection_score = injection.captures[0].start;
        if injection_score < match_score || (priority_match && injection_score == match_score) {
            return Some(injection);
        }
        Some(match_result)
    }

    fn match_injections(
        &self,
        line: &str,
        is_first_line: bool,
        line_pos: usize,
        stack: &StateStack,
        allow_g: bool,
    ) -> Option<(MatchResult, bool)> {
        let mut best: Option<(MatchResult, i32)> = None;
        let scopes = stack.content_name_scopes_list.scope_names();
        for injection in &self.grammar.injections {
            if !injection.matcher.matches(&scopes) {
                continue;
            }
            let scanner = self.grammar.scanner(
                injection.rule_id,
                ListKind::Patterns,
                None,
                is_first_line,
                allow_g,
            );
            let Some(m) = self.find(&scanner, line, line_pos) else {
                continue;
            };
            let rating = m.captures[0].start;
            if best
                .as_ref()
                .is_some_and(|(b, _)| rating >= b.captures[0].start)
            {
                continue;
            }
            best = Some((
                MatchResult {
                    rule_id: scanner.rule_ids[m.index],
                    captures: m.captures,
                },
                injection.priority,
            ));
            if rating == line_pos {
                break;
            }
        }
        best.map(|(m, priority)| (m, priority == -1))
    }

    /// Port of `handleCaptures`.
    fn handle_captures(
        &self,
        line: &str,
        is_first_line: bool,
        stack: &Arc<StateStack>,
        tokens: &mut LineTokens,
        captures: &[Option<RuleId>],
        capture_indices: &[CaptureIndex],
    ) {
        if captures.is_empty() {
            return;
        }
        let len = captures.len().min(capture_indices.len());
        let mut local_stack: Vec<(Arc<AttributedScopeStack>, usize)> = Vec::new();
        let max_end = capture_indices[0].end;
        for i in 0..len {
            let Some(capture_rule_id) = captures[i] else {
                continue;
            };
            let capture = capture_indices[i];
            if capture.length == 0 {
                continue;
            }
            if capture.start > max_end {
                break;
            }
            while let Some((scopes, end)) = local_stack.last() {
                if *end > capture.start {
                    break;
                }
                tokens.produce_from_scopes(scopes, *end);
                local_stack.pop();
            }
            match local_stack.last() {
                Some((scopes, _)) => tokens.produce_from_scopes(scopes, capture.start),
                None => tokens.produce(stack, capture.start),
            }
            let capture_rule = self.grammar.rule(capture_rule_id);
            let RuleKind::Capture { retokenize_with } = capture_rule.kind else {
                continue;
            };
            if retokenize_with != 0 {
                let scope_name = capture_rule.get_name(Some(line), Some(capture_indices));
                let name_scopes_list = stack
                    .content_name_scopes_list
                    .push_attributed(scope_name.as_deref(), self.theme);
                let content_name = capture_rule.get_content_name(line, capture_indices);
                let content_list =
                    name_scopes_list.push_attributed(content_name.as_deref(), self.theme);
                let stack_clone = stack.push(
                    retokenize_with,
                    capture.start as i64,
                    -1,
                    self.line_id,
                    false,
                    None,
                    name_scopes_list,
                    content_list,
                );
                let sub = &line[..capture.end];
                self.tokenize_string(
                    sub,
                    is_first_line && capture.start == 0,
                    capture.start,
                    stack_clone,
                    tokens,
                    false,
                    false,
                );
                continue;
            }
            if let Some(name) = capture_rule.get_name(Some(line), Some(capture_indices)) {
                let base = match local_stack.last() {
                    Some((scopes, _)) => scopes.clone(),
                    None => stack.content_name_scopes_list.clone(),
                };
                let scopes = base.push_attributed(Some(&name), self.theme);
                local_stack.push((scopes, capture.end));
            }
        }
        while let Some((scopes, end)) = local_stack.pop() {
            tokens.produce_from_scopes(&scopes, end);
        }
    }
}

/// Builds grammars from raw grammars.
pub(crate) fn build(root: Arc<RawGrammar>, lookup: &dyn GrammarLookup) -> Grammar {
    Grammar::new(root, lookup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_types() {
        assert_eq!(standard_token_type("comment.line.js"), 1);
        assert_eq!(standard_token_type("string.quoted"), 2);
        assert_eq!(standard_token_type("meta.embedded.block"), 0);
        assert_eq!(standard_token_type("substring.x"), TOKEN_TYPE_NOT_SET);
        assert_eq!(standard_token_type("keyword"), TOKEN_TYPE_NOT_SET);
    }
}
