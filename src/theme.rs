//! VS Code themes: loading, Shiki normalization and TextMate scope matching.
//!
//! The scope matching is a port of `theme.ts` from vscode-textmate. The
//! normalization follows `normalizeTheme` from Shiki.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use crate::error::{Error, Result};

/// Font style bit flags, as in vscode-textmate.
pub mod font_style {
    /// No font style.
    pub const NONE: u8 = 0;
    /// Italic text.
    pub const ITALIC: u8 = 1;
    /// Bold text.
    pub const BOLD: u8 = 2;
    /// Underlined text.
    pub const UNDERLINE: u8 = 4;
    /// Struck through text.
    pub const STRIKETHROUGH: u8 = 8;
}

/// `FontStyle.NotSet` of vscode-textmate.
pub(crate) const FONT_STYLE_NOT_SET: i8 = -1;

const VSCODE_FALLBACK_EDITOR_FG_LIGHT: &str = "#333333";
const VSCODE_FALLBACK_EDITOR_FG_DARK: &str = "#bbbbbb";
const VSCODE_FALLBACK_EDITOR_BG_LIGHT: &str = "#fffffe";
const VSCODE_FALLBACK_EDITOR_BG_DARK: &str = "#1e1e1e";

/// The scope of a theme setting: one string (comma separated) or a list.
#[derive(Clone, Debug)]
pub enum ThemeScope {
    /// A comma separated list of selectors.
    One(String),
    /// A list of selectors.
    Many(Vec<String>),
}

/// The style part of a theme setting.
#[derive(Clone, Debug, Default)]
pub struct ThemeStyle {
    /// Foreground color.
    pub foreground: Option<String>,
    /// Background color.
    pub background: Option<String>,
    /// Space separated font styles (`"italic bold"`), `Some("")` resets the style.
    pub font_style: Option<String>,
}

/// One entry of `tokenColors` / `settings`.
#[derive(Clone, Debug, Default)]
pub struct ThemeSetting {
    /// Optional name of the setting.
    pub name: Option<String>,
    /// Scope selectors this setting applies to.
    pub scope: Option<ThemeScope>,
    /// The style, `None` when the entry has no `settings` object.
    pub settings: Option<ThemeStyle>,
}

/// A normalized theme, ready for highlighting.
#[derive(Debug)]
pub struct Theme {
    /// Name of the theme, used to select it and as a CSS class.
    pub name: String,
    /// Display name, when the theme provides one.
    pub display_name: Option<String>,
    /// `"light"` or `"dark"`.
    pub kind: String,
    /// Default foreground color.
    pub fg: String,
    /// Default background color.
    pub bg: String,
    /// Token color rules.
    pub settings: Vec<ThemeSetting>,
    /// Workbench colors (`editor.background`, `terminal.ansiRed`...).
    pub colors: HashMap<String, String>,
    /// Colors replaced in the output, keyed by lowercase color.
    pub color_replacements: HashMap<String, String>,
    pub(crate) textmate: TextMateTheme,
}

impl Theme {
    /// Parses a theme from JSON (a VS Code theme or a Shiki theme registration).
    pub fn from_json(json: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(json)?;
        let Value::Object(obj) = value else {
            return Err(invalid("a theme must be a JSON object"));
        };
        Self::from_object(&obj)
    }

    /// Parses a theme from a JSON object.
    pub fn from_object(obj: &Map<String, Value>) -> Result<Self> {
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("missing `name`"))?
            .to_string();
        let settings_value = match obj.get("settings") {
            Some(v @ Value::Array(_)) => Some(v),
            _ => obj.get("tokenColors"),
        };
        let settings = match settings_value {
            Some(Value::Array(items)) => items.iter().filter_map(parse_setting).collect(),
            _ => Vec::new(),
        };
        let colors = string_map(obj.get("colors"));
        let color_replacements = string_map(obj.get("colorReplacements"));
        let kind = obj
            .get("type")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("dark")
            .to_string();
        Ok(Self::normalize(RawThemeParts {
            name,
            display_name: obj
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::to_string),
            kind,
            fg: obj
                .get("fg")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            bg: obj
                .get("bg")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            settings,
            colors,
            color_replacements,
        }))
    }

    /// Port of Shiki's `normalizeTheme`.
    fn normalize(raw: RawThemeParts) -> Self {
        let RawThemeParts {
            name,
            display_name,
            kind,
            mut fg,
            mut bg,
            mut settings,
            mut colors,
            mut color_replacements,
        } = raw;
        if fg.is_none() || bg.is_none() {
            let global = settings
                .iter()
                .find(|s| s.name.as_deref().is_none_or(str::is_empty) && scope_is_falsy(&s.scope));
            let global_style = global.and_then(|g| g.settings.as_ref());
            let mut new_fg = global_style
                .and_then(|s| s.foreground.clone())
                .filter(|s| !s.is_empty());
            let mut new_bg = global_style
                .and_then(|s| s.background.clone())
                .filter(|s| !s.is_empty());
            if new_fg.is_none() {
                new_fg = colors
                    .get("editor.foreground")
                    .filter(|s| !s.is_empty())
                    .cloned();
            }
            if new_bg.is_none() {
                new_bg = colors
                    .get("editor.background")
                    .filter(|s| !s.is_empty())
                    .cloned();
            }
            let light = kind == "light";
            fg = Some(new_fg.unwrap_or_else(|| {
                if light {
                    VSCODE_FALLBACK_EDITOR_FG_LIGHT
                } else {
                    VSCODE_FALLBACK_EDITOR_FG_DARK
                }
                .to_string()
            }));
            bg = Some(new_bg.unwrap_or_else(|| {
                if light {
                    VSCODE_FALLBACK_EDITOR_BG_LIGHT
                } else {
                    VSCODE_FALLBACK_EDITOR_BG_DARK
                }
                .to_string()
            }));
        }
        let fg = fg.unwrap_or_default();
        let bg = bg.unwrap_or_default();
        let first_is_global = settings
            .first()
            .is_some_and(|s| s.settings.is_some() && scope_is_falsy(&s.scope));
        if !first_is_global {
            settings.insert(
                0,
                ThemeSetting {
                    name: None,
                    scope: None,
                    settings: Some(ThemeStyle {
                        foreground: Some(fg.clone()),
                        background: Some(bg.clone()),
                        font_style: None,
                    }),
                },
            );
        }

        // Colors that are not hex (like CSS variables) are replaced by
        // placeholders, and restored through `color_replacements`.
        let mut count = 0u32;
        let mut map: HashMap<String, String> = HashMap::new();
        let mut placeholder = |value: &str, replacements: &HashMap<String, String>| -> String {
            if let Some(hex) = map.get(value) {
                return hex.clone();
            }
            loop {
                count += 1;
                let hex = format!("#{count:08x}");
                if replacements.contains_key(&format!("#{hex}")) {
                    continue;
                }
                map.insert(value.to_string(), hex.clone());
                return hex;
            }
        };
        for setting in &mut settings {
            let Some(style) = setting.settings.as_mut() else {
                continue;
            };
            for color in [&mut style.foreground, &mut style.background] {
                if let Some(value) = color
                    .as_ref()
                    .filter(|c| !c.is_empty() && !c.starts_with('#'))
                {
                    let value = value.clone();
                    let hex = placeholder(&value, &color_replacements);
                    color_replacements.insert(hex.clone(), value);
                    *color = Some(hex);
                }
            }
        }
        let mut keys: Vec<String> = colors.keys().cloned().collect();
        keys.sort();
        for key in keys {
            if key == "editor.foreground"
                || key == "editor.background"
                || key.starts_with("terminal.ansi")
            {
                let value = colors[&key].clone();
                if !value.starts_with('#') {
                    let hex = placeholder(&value, &color_replacements);
                    color_replacements.insert(hex.clone(), value);
                    colors.insert(key, hex);
                }
            }
        }

        let textmate = TextMateTheme::new(&settings);
        Self {
            name,
            display_name,
            kind,
            fg,
            bg,
            settings,
            colors,
            color_replacements,
            textmate,
        }
    }

    /// Applies `color_replacements` to a color.
    pub(crate) fn replace_color<'a>(
        &'a self,
        color: &'a str,
        extra: &'a HashMap<String, String>,
    ) -> &'a str {
        let key = color.to_lowercase();
        extra
            .get(&key)
            .or_else(|| self.color_replacements.get(&key))
            .map(String::as_str)
            .unwrap_or(color)
    }
}

struct RawThemeParts {
    name: String,
    display_name: Option<String>,
    kind: String,
    fg: Option<String>,
    bg: Option<String>,
    settings: Vec<ThemeSetting>,
    colors: HashMap<String, String>,
    color_replacements: HashMap<String, String>,
}

fn invalid(message: &str) -> Error {
    Error::Invalid {
        kind: "theme",
        message: message.to_string(),
    }
}

fn scope_is_falsy(scope: &Option<ThemeScope>) -> bool {
    match scope {
        None => true,
        Some(ThemeScope::One(s)) => s.is_empty(),
        Some(ThemeScope::Many(_)) => false,
    }
}

fn string_map(value: Option<&Value>) -> HashMap<String, String> {
    match value {
        Some(Value::Object(map)) => map
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
            .collect(),
        _ => HashMap::new(),
    }
}

fn parse_setting(value: &Value) -> Option<ThemeSetting> {
    let obj = value.as_object()?;
    let scope = match obj.get("scope") {
        Some(Value::String(s)) => Some(ThemeScope::One(s.clone())),
        Some(Value::Array(items)) => Some(ThemeScope::Many(
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        )),
        _ => None,
    };
    let settings = match obj.get("settings") {
        Some(Value::Object(s)) => Some(ThemeStyle {
            foreground: s
                .get("foreground")
                .and_then(Value::as_str)
                .map(str::to_string),
            background: s
                .get("background")
                .and_then(Value::as_str)
                .map(str::to_string),
            font_style: s
                .get("fontStyle")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        _ => None,
    };
    Some(ThemeSetting {
        name: obj.get("name").and_then(Value::as_str).map(str::to_string),
        scope,
        settings,
    })
}

// ---------------------------------------------------------------------------
// TextMate theme (port of vscode-textmate `theme.ts`)
// ---------------------------------------------------------------------------

/// Style resolved for a scope path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StyleAttributes {
    pub font_style: i8,
    pub foreground_id: u32,
    pub background_id: u32,
}

#[derive(Clone, Debug)]
struct TrieRule {
    scope_depth: usize,
    parent_scopes: Arc<[String]>,
    font_style: i8,
    foreground: u32,
    background: u32,
}

impl TrieRule {
    fn accept_overwrite(
        &mut self,
        scope_depth: usize,
        font_style: i8,
        foreground: u32,
        background: u32,
    ) {
        if self.scope_depth <= scope_depth {
            self.scope_depth = scope_depth;
        }
        if font_style != FONT_STYLE_NOT_SET {
            self.font_style = font_style;
        }
        if foreground != 0 {
            self.foreground = foreground;
        }
        if background != 0 {
            self.background = background;
        }
    }
}

#[derive(Debug)]
struct TrieElement {
    main_rule: TrieRule,
    rules_with_parent_scopes: Vec<TrieRule>,
    children: HashMap<String, TrieElement>,
}

impl TrieElement {
    fn new(main_rule: TrieRule, rules_with_parent_scopes: Vec<TrieRule>) -> Self {
        Self {
            main_rule,
            rules_with_parent_scopes,
            children: HashMap::new(),
        }
    }

    fn match_scope(&self, scope: &str) -> Vec<TrieRule> {
        if !scope.is_empty() {
            let (head, tail) = scope.split_once('.').unwrap_or((scope, ""));
            if let Some(child) = self.children.get(head) {
                return child.match_scope(tail);
            }
        }
        let mut rules = self.rules_with_parent_scopes.clone();
        rules.push(self.main_rule.clone());
        rules.sort_by(cmp_by_specificity);
        rules
    }

    fn insert(
        &mut self,
        scope_depth: usize,
        scope: &str,
        parent_scopes: Option<&Arc<[String]>>,
        font_style: i8,
        foreground: u32,
        background: u32,
    ) {
        if scope.is_empty() {
            self.insert_here(
                scope_depth,
                parent_scopes,
                font_style,
                foreground,
                background,
            );
            return;
        }
        let (head, tail) = scope.split_once('.').unwrap_or((scope, ""));
        let child = self.children.entry(head.to_string()).or_insert_with(|| {
            TrieElement::new(
                self.main_rule.clone(),
                self.rules_with_parent_scopes.clone(),
            )
        });
        child.insert(
            scope_depth + 1,
            tail,
            parent_scopes,
            font_style,
            foreground,
            background,
        );
    }

    fn insert_here(
        &mut self,
        scope_depth: usize,
        parent_scopes: Option<&Arc<[String]>>,
        mut font_style: i8,
        mut foreground: u32,
        mut background: u32,
    ) {
        let Some(parent_scopes) = parent_scopes else {
            self.main_rule
                .accept_overwrite(scope_depth, font_style, foreground, background);
            return;
        };
        for rule in &mut self.rules_with_parent_scopes {
            if *rule.parent_scopes == **parent_scopes {
                rule.accept_overwrite(scope_depth, font_style, foreground, background);
                return;
            }
        }
        if font_style == FONT_STYLE_NOT_SET {
            font_style = self.main_rule.font_style;
        }
        if foreground == 0 {
            foreground = self.main_rule.foreground;
        }
        if background == 0 {
            background = self.main_rule.background;
        }
        self.rules_with_parent_scopes.push(TrieRule {
            scope_depth,
            parent_scopes: parent_scopes.clone(),
            font_style,
            foreground,
            background,
        });
    }
}

fn cmp_by_specificity(a: &TrieRule, b: &TrieRule) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a.scope_depth != b.scope_depth {
        return b.scope_depth.cmp(&a.scope_depth);
    }
    let (mut ai, mut bi) = (0, 0);
    loop {
        if a.parent_scopes.get(ai).map(String::as_str) == Some(">") {
            ai += 1;
        }
        if b.parent_scopes.get(bi).map(String::as_str) == Some(">") {
            bi += 1;
        }
        if ai >= a.parent_scopes.len() || bi >= b.parent_scopes.len() {
            break;
        }
        let diff = b.parent_scopes[bi].len() as i64 - a.parent_scopes[ai].len() as i64;
        if diff != 0 {
            return if diff < 0 {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        ai += 1;
        bi += 1;
    }
    b.parent_scopes.len().cmp(&a.parent_scopes.len())
}

/// A linked list of scope names, innermost last (port of `ScopeStack`).
#[derive(Debug)]
pub(crate) struct ScopeStack {
    pub parent: Option<Arc<ScopeStack>>,
    pub scope_name: String,
}

impl ScopeStack {
    pub fn push(self: &Arc<Self>, scope_name: &str) -> Arc<Self> {
        Arc::new(Self {
            parent: Some(self.clone()),
            scope_name: scope_name.to_string(),
        })
    }

    pub fn segments(&self) -> Vec<&str> {
        let mut out = Vec::new();
        let mut item = Some(self);
        while let Some(s) = item {
            out.push(s.scope_name.as_str());
            item = s.parent.as_deref();
        }
        out.reverse();
        out
    }
}

/// The compiled scope matching of a theme.
#[derive(Debug)]
pub(crate) struct TextMateTheme {
    color_map: Vec<String>,
    defaults: StyleAttributes,
    root: TrieElement,
    cache: Mutex<HashMap<String, Arc<[TrieRule]>>>,
}

struct ParsedRule {
    scope: String,
    parent_scopes: Option<Arc<[String]>>,
    index: usize,
    font_style: i8,
    foreground: Option<String>,
    background: Option<String>,
}

impl TextMateTheme {
    fn new(settings: &[ThemeSetting]) -> Self {
        let mut rules = parse_theme(settings);
        rules.sort_by(|a, b| {
            a.scope
                .cmp(&b.scope)
                .then_with(|| str_arr_cmp(a.parent_scopes.as_deref(), b.parent_scopes.as_deref()))
                .then_with(|| a.index.cmp(&b.index))
        });
        let mut default_font_style = font_style::NONE as i8;
        let mut default_foreground = "#000000".to_string();
        let mut default_background = "#ffffff".to_string();
        let mut start = 0;
        while start < rules.len() && rules[start].scope.is_empty() {
            let incoming = &rules[start];
            if incoming.font_style != FONT_STYLE_NOT_SET {
                default_font_style = incoming.font_style;
            }
            if let Some(fg) = &incoming.foreground {
                default_foreground = fg.clone();
            }
            if let Some(bg) = &incoming.background {
                default_background = bg.clone();
            }
            start += 1;
        }
        let mut color_map = ColorMap::default();
        let defaults = StyleAttributes {
            font_style: default_font_style,
            foreground_id: color_map.get_id(Some(&default_foreground)),
            background_id: color_map.get_id(Some(&default_background)),
        };
        let mut root = TrieElement::new(
            TrieRule {
                scope_depth: 0,
                parent_scopes: Arc::from(Vec::new()),
                font_style: FONT_STYLE_NOT_SET,
                foreground: 0,
                background: 0,
            },
            Vec::new(),
        );
        for rule in &rules[start..] {
            let fg = color_map.get_id(rule.foreground.as_deref());
            let bg = color_map.get_id(rule.background.as_deref());
            root.insert(
                0,
                &rule.scope,
                rule.parent_scopes.as_ref(),
                rule.font_style,
                fg,
                bg,
            );
        }
        Self {
            color_map: color_map.id_to_color,
            defaults,
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Colors by id. Id 0 is unused.
    pub fn color_map(&self) -> &[String] {
        &self.color_map
    }

    pub fn defaults(&self) -> StyleAttributes {
        self.defaults
    }

    /// Resolves the style of the innermost scope of `path`.
    pub fn match_path(&self, path: &ScopeStack) -> Option<StyleAttributes> {
        let rules = {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            match cache.get(&path.scope_name) {
                Some(rules) => rules.clone(),
                None => {
                    let rules: Arc<[TrieRule]> = self.root.match_scope(&path.scope_name).into();
                    cache.insert(path.scope_name.clone(), rules.clone());
                    rules
                }
            }
        };
        let rule = rules
            .iter()
            .find(|r| scope_path_matches_parent_scopes(path.parent.as_deref(), &r.parent_scopes))?;
        Some(StyleAttributes {
            font_style: rule.font_style,
            foreground_id: rule.foreground,
            background_id: rule.background,
        })
    }
}

fn scope_path_matches_parent_scopes(
    mut path: Option<&ScopeStack>,
    parent_scopes: &[String],
) -> bool {
    if parent_scopes.is_empty() {
        return true;
    }
    let mut index = 0;
    while index < parent_scopes.len() {
        let mut pattern = parent_scopes[index].as_str();
        let mut must_match = false;
        if pattern == ">" {
            if index == parent_scopes.len() - 1 {
                return false;
            }
            index += 1;
            pattern = parent_scopes[index].as_str();
            must_match = true;
        }
        while let Some(p) = path {
            if matches_scope(&p.scope_name, pattern) {
                break;
            }
            if must_match {
                return false;
            }
            path = p.parent.as_deref();
        }
        let Some(p) = path else { return false };
        path = p.parent.as_deref();
        index += 1;
    }
    true
}

fn matches_scope(scope_name: &str, pattern: &str) -> bool {
    pattern == scope_name
        || (scope_name.starts_with(pattern)
            && scope_name.as_bytes().get(pattern.len()) == Some(&b'.'))
}

fn str_arr_cmp(a: Option<&[String]>, b: Option<&[String]>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) => {
            if a.len() == b.len() {
                for (x, y) in a.iter().zip(b) {
                    let r = x.cmp(y);
                    if r != Ordering::Equal {
                        return r;
                    }
                }
                Ordering::Equal
            } else {
                a.len().cmp(&b.len())
            }
        }
    }
}

fn is_valid_hex_color(hex: &str) -> bool {
    let Some(digits) = hex.strip_prefix('#') else {
        return false;
    };
    matches!(digits.len(), 3 | 4 | 6 | 8) && digits.bytes().all(|b| b.is_ascii_hexdigit())
}

fn parse_theme(settings: &[ThemeSetting]) -> Vec<ParsedRule> {
    let mut result = Vec::new();
    for (index, entry) in settings.iter().enumerate() {
        let Some(style) = &entry.settings else {
            continue;
        };
        let scopes: Vec<String> = match &entry.scope {
            Some(ThemeScope::One(s)) => {
                let trimmed = s.trim_start_matches(',').trim_end_matches(',');
                trimmed.split(',').map(str::to_string).collect()
            }
            Some(ThemeScope::Many(list)) => list.clone(),
            None => vec![String::new()],
        };
        let mut font_style = FONT_STYLE_NOT_SET;
        if let Some(fs) = &style.font_style {
            font_style = font_style::NONE as i8;
            for segment in fs.split(' ') {
                font_style |= match segment {
                    "italic" => font_style::ITALIC,
                    "bold" => font_style::BOLD,
                    "underline" => font_style::UNDERLINE,
                    "strikethrough" => font_style::STRIKETHROUGH,
                    _ => 0,
                } as i8;
            }
        }
        let foreground = style.foreground.clone().filter(|c| is_valid_hex_color(c));
        let background = style.background.clone().filter(|c| is_valid_hex_color(c));
        for scope in scopes {
            let scope = scope.trim();
            let segments: Vec<&str> = scope.split(' ').collect();
            let last = segments[segments.len() - 1].to_string();
            let parent_scopes = if segments.len() > 1 {
                let mut parents: Vec<String> = segments[..segments.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                parents.reverse();
                Some(Arc::from(parents))
            } else {
                None
            };
            result.push(ParsedRule {
                scope: last,
                parent_scopes,
                index,
                font_style,
                foreground: foreground.clone(),
                background: background.clone(),
            });
        }
    }
    result
}

#[derive(Default)]
struct ColorMap {
    id_to_color: Vec<String>,
    color_to_id: HashMap<String, u32>,
}

impl ColorMap {
    fn get_id(&mut self, color: Option<&str>) -> u32 {
        let Some(color) = color else { return 0 };
        if self.id_to_color.is_empty() {
            self.id_to_color.push(String::new());
        }
        let color = color.to_uppercase();
        if let Some(id) = self.color_to_id.get(&color) {
            return *id;
        }
        let id = self.id_to_color.len() as u32;
        self.id_to_color.push(color.clone());
        self.color_to_id.insert(color, id);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(scopes: &[&str]) -> Arc<ScopeStack> {
        let mut p = Arc::new(ScopeStack {
            parent: None,
            scope_name: scopes[0].to_string(),
        });
        for s in &scopes[1..] {
            p = p.push(s);
        }
        p
    }

    #[test]
    fn matches_most_specific_rule() {
        let theme = Theme::from_json(
            r##"{"name":"t","type":"dark","colors":{"editor.foreground":"#111111","editor.background":"#000000"},
            "tokenColors":[
              {"scope":"string","settings":{"foreground":"#aa0000"}},
              {"scope":"string.quoted","settings":{"foreground":"#bb0000","fontStyle":"italic"}},
              {"scope":"source.js string","settings":{"foreground":"#cc0000"}},
              {"scope":["comment", "punctuation.definition.comment"],"settings":{"fontStyle":"bold"}}
            ]}"##,
        )
        .unwrap();
        assert_eq!(theme.fg, "#111111");
        assert_eq!(theme.bg, "#000000");
        let tm = &theme.textmate;
        let color = |p: &[&str]| {
            tm.match_path(&path(p))
                .map(|s| tm.color_map()[s.foreground_id as usize].clone())
        };
        assert_eq!(
            color(&["source.rust", "string.quoted.double"]).as_deref(),
            Some("#BB0000")
        );
        assert_eq!(
            color(&["source.js", "string.unquoted"]).as_deref(),
            Some("#CC0000")
        );
        assert_eq!(
            color(&["source.rust", "string.unquoted"]).as_deref(),
            Some("#AA0000")
        );
        let s = tm
            .match_path(&path(&["source.rust", "comment.line"]))
            .unwrap();
        assert_eq!(s.font_style, font_style::BOLD as i8);
    }
}
