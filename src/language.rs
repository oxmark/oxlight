//! Language registrations (a TextMate grammar plus Shiki metadata).

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::grammar::raw::RawGrammar;

/// A language: a TextMate grammar and the metadata Shiki attaches to it.
///
/// The JSON format is the one of Shiki language registrations (as found in
/// `@shikijs/langs` or `tm-grammars`), which is a TextMate grammar with a few
/// extra fields: `name`, `aliases`, `embeddedLangs` and `injectTo`.
#[derive(Debug, Clone)]
pub struct Language {
    /// Identifier of the language (`"rust"`).
    pub name: String,
    /// Human readable name (`"Rust"`).
    pub display_name: Option<String>,
    /// Root scope of the grammar (`"source.rust"`).
    pub scope_name: String,
    /// Other names the language can be selected with (`"rs"`).
    pub aliases: Vec<String>,
    /// Languages this grammar embeds, which must also be loaded.
    pub embedded_langs: Vec<String>,
    /// Scopes this grammar injects itself into.
    pub inject_to: Vec<String>,
    pub(crate) grammar: Arc<RawGrammar>,
}

impl Language {
    /// Parses one language registration from JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(json)?;
        match value {
            Value::Object(obj) => Self::from_object(&obj),
            _ => Err(invalid("a language must be a JSON object")),
        }
    }

    /// Parses a JSON document holding one language or an array of languages.
    pub fn many_from_json(json: &str) -> Result<Vec<Self>> {
        let value: Value = serde_json::from_str(json)?;
        match value {
            Value::Object(obj) => Ok(vec![Self::from_object(&obj)?]),
            Value::Array(items) => items
                .iter()
                .map(|item| match item {
                    Value::Object(obj) => Self::from_object(obj),
                    _ => Err(invalid("a language must be a JSON object")),
                })
                .collect(),
            _ => Err(invalid(
                "expected a language object or an array of languages",
            )),
        }
    }

    /// Parses a language registration from a JSON object.
    pub fn from_object(obj: &Map<String, Value>) -> Result<Self> {
        let grammar = RawGrammar::from_json(obj).map_err(|m| invalid(&m))?;
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| invalid("missing `name`"))?;
        Ok(Self {
            name,
            display_name: obj
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::to_string),
            scope_name: grammar.scope_name.clone(),
            aliases: strings(obj.get("aliases")),
            embedded_langs: {
                let mut langs = strings(obj.get("embeddedLangs"));
                if langs.is_empty() {
                    langs = strings(obj.get("embeddedLanguages"));
                }
                langs
            },
            inject_to: strings(obj.get("injectTo")),
            grammar: Arc::new(grammar),
        })
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn invalid(message: &str) -> Error {
    Error::Invalid {
        kind: "language",
        message: message.to_string(),
    }
}
