//! The highlighter: loaded languages and themes, and the entry points.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};
use crate::grammar::{self, raw::RawGrammar, Grammar, GrammarLookup};
use crate::language::Language;
use crate::options::HighlightOptions;
use crate::theme::Theme;
use crate::tokens::{self, TokensResult};

/// Holds languages and themes, and highlights code with them.
///
/// Grammars are compiled lazily on first use and cached. Loading a language
/// clears that cache, because it can change what other grammars include.
///
/// `Highlighter` is `Send + Sync`: it can be shared between threads once loaded.
#[derive(Default)]
pub struct Highlighter {
    languages: HashMap<String, Arc<Language>>,
    /// Language names in loading order (first load), for injection order.
    language_order: Vec<String>,
    aliases: HashMap<String, String>,
    by_scope: HashMap<String, Arc<Language>>,
    themes: HashMap<String, Arc<Theme>>,
    theme_order: Vec<String>,
    grammars: Mutex<HashMap<String, Arc<Grammar>>>,
}

impl std::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter")
            .field("languages", &self.languages.keys().collect::<Vec<_>>())
            .field("themes", &self.theme_order)
            .finish()
    }
}

/// Language names that are rendered without highlighting.
pub(crate) fn is_plain_lang(lang: &str) -> bool {
    matches!(lang, "" | "plaintext" | "txt" | "text" | "plain")
}

impl Highlighter {
    /// An empty highlighter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a language.
    pub fn load_language(&mut self, language: Language) {
        let language = Arc::new(language);
        for alias in &language.aliases {
            self.aliases.insert(alias.clone(), language.name.clone());
        }
        self.by_scope
            .insert(language.scope_name.clone(), language.clone());
        if !self.languages.contains_key(&language.name) {
            self.language_order.push(language.name.clone());
        }
        self.languages.insert(language.name.clone(), language);
        self.grammars
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Loads languages from JSON: one registration or an array of them.
    ///
    /// Returns the names of the loaded languages.
    pub fn load_language_json(&mut self, json: &str) -> Result<Vec<String>> {
        let languages = Language::many_from_json(json)?;
        let names = languages.iter().map(|l| l.name.clone()).collect();
        for language in languages {
            self.load_language(language);
        }
        Ok(names)
    }

    /// Loads a theme. A theme with the same name is replaced.
    pub fn load_theme(&mut self, theme: Theme) {
        if !self.themes.contains_key(&theme.name) {
            self.theme_order.push(theme.name.clone());
        }
        self.themes.insert(theme.name.clone(), Arc::new(theme));
    }

    /// Loads a theme from JSON and returns its name.
    pub fn load_theme_json(&mut self, json: &str) -> Result<String> {
        let theme = Theme::from_json(json)?;
        let name = theme.name.clone();
        self.load_theme(theme);
        Ok(name)
    }

    /// Names of the loaded languages, sorted.
    pub fn loaded_languages(&self) -> Vec<String> {
        let mut names: Vec<String> = self.languages.keys().cloned().collect();
        names.sort();
        names
    }

    /// Names of the loaded themes, in loading order.
    pub fn loaded_themes(&self) -> Vec<String> {
        self.theme_order.clone()
    }

    /// Looks up a language by name or alias.
    pub fn language(&self, name: &str) -> Option<&Language> {
        self.languages
            .get(&self.resolve_alias(name))
            .map(Arc::as_ref)
    }

    /// Looks up a theme by name.
    pub fn theme(&self, name: &str) -> Option<&Theme> {
        self.themes.get(name).map(Arc::as_ref)
    }

    pub(crate) fn theme_arc(&self, name: &str) -> Result<Arc<Theme>> {
        self.themes
            .get(name)
            .cloned()
            .ok_or_else(|| Error::UnknownTheme(name.to_string()))
    }

    /// Port of Shiki's `resolveLangAlias`, with cycle protection.
    pub(crate) fn resolve_alias(&self, name: &str) -> String {
        let mut name = name.to_string();
        let mut seen = vec![name.clone()];
        while let Some(next) = self.aliases.get(&name) {
            if seen.contains(next) {
                break;
            }
            name = next.clone();
            seen.push(name.clone());
        }
        name
    }

    /// The compiled grammar of a language.
    pub(crate) fn grammar(&self, lang: &str) -> Result<Arc<Grammar>> {
        let language = self
            .languages
            .get(&self.resolve_alias(lang))
            .ok_or_else(|| Error::UnknownLanguage(lang.to_string()))?;
        let mut grammars = self.grammars.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(g) = grammars.get(&language.scope_name) {
            return Ok(g.clone());
        }
        let g = Arc::new(grammar::build(language.grammar.clone(), self));
        grammars.insert(language.scope_name.clone(), g.clone());
        Ok(g)
    }

    /// Tokenizes `code` and resolves token colors with the selected theme(s).
    pub fn code_to_tokens(&self, code: &str, options: &HighlightOptions) -> Result<TokensResult> {
        tokens::code_to_tokens(self, code, options)
    }

    /// Highlights `code` to HTML, like Shiki's `codeToHtml`.
    pub fn code_to_html(&self, code: &str, options: &HighlightOptions) -> Result<String> {
        let result = self.code_to_tokens(code, options)?;
        Ok(crate::html::tokens_to_html(&result, options))
    }
}

impl GrammarLookup for Highlighter {
    fn lookup(&self, scope_name: &str) -> Option<Arc<RawGrammar>> {
        self.by_scope.get(scope_name).map(|l| l.grammar.clone())
    }

    /// Port of Shiki's `Resolver.getInjections`: injections registered for
    /// any dotted prefix of the scope.
    fn injections(&self, scope_name: &str) -> Vec<String> {
        let mut out = Vec::new();
        let parts: Vec<&str> = scope_name.split('.').collect();
        let languages: Vec<&Arc<Language>> = self
            .language_order
            .iter()
            .filter_map(|n| self.languages.get(n))
            .collect();
        for i in 1..=parts.len() {
            let prefix = parts[..i].join(".");
            for language in &languages {
                if language.inject_to.contains(&prefix) {
                    out.push(language.scope_name.clone());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighter_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<Highlighter>();
    }
}
