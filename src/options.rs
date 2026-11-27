//! Options of the highlighting functions.

use std::collections::HashMap;
use std::time::Duration;

/// Which theme(s) to highlight with.
#[derive(Clone, Debug)]
pub enum ThemeSelection {
    /// One theme, by name.
    Single(String),
    /// Several themes (light/dark...), rendered with CSS variables.
    Multiple {
        /// `(color key, theme name)` pairs, like `("light", "github-light")`.
        themes: Vec<(String, String)>,
        /// Which key gets the plain `color`/`background-color` styles.
        default_color: DefaultColor,
        /// Prefix of the CSS variables (`"--shiki-"` by default).
        css_variable_prefix: String,
        /// Whether other themes are rendered as CSS variables.
        colors_rendering: ColorsRendering,
    },
}

/// The theme whose colors are written as plain CSS properties.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefaultColor {
    /// The theme with this key (Shiki's default is `"light"`).
    Key(String),
    /// Use CSS `light-dark()` with the `light` and `dark` themes.
    LightDark,
    /// No default: only CSS variables are written.
    None,
}

/// How the themes other than the default one are rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorsRendering {
    /// As CSS variables (`--shiki-dark:#fff`), Shiki's default.
    CssVars,
    /// Not rendered.
    None,
}

/// How whitespace-only tokens are merged (Shiki's `mergeWhitespaces`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeWhitespaces {
    /// Merge whitespace tokens into the next token (default).
    Merge,
    /// Keep tokens as produced by the grammar.
    Keep,
    /// Split leading and trailing whitespace into separate tokens.
    Never,
}

/// Options shared by tokenization and HTML rendering.
#[derive(Clone, Debug)]
pub struct HighlightOptions {
    /// Language name or alias. `text`, `plaintext`, `txt` and `plain` disable highlighting.
    pub lang: String,
    /// Theme(s) to use.
    pub theme: ThemeSelection,
    /// Color replacements applied to the output, keyed by lowercase color.
    pub color_replacements: HashMap<String, String>,
    /// Lines at least this long (in UTF-16 code units, like Shiki) are not tokenized. 0 disables the limit.
    pub tokenize_max_line_length: usize,
    /// Time limit for tokenizing one line. `None` disables the limit. Shiki's default is 500 ms.
    pub tokenize_time_limit: Option<Duration>,
    /// Whitespace merging, see [`MergeWhitespaces`].
    pub merge_whitespaces: MergeWhitespaces,
    /// Merge adjacent tokens with the same style.
    pub merge_same_style_tokens: bool,
    /// The `tabindex` attribute of `<pre>`. `None` removes it.
    pub tabindex: Option<String>,
    /// Whether the `<pre>` gets a `style` attribute with the theme colors.
    pub root_style: bool,
    /// Record TextMate scopes on each token (see [`crate::ThemedToken::scopes`]).
    pub include_scopes: bool,
}

impl HighlightOptions {
    /// Options for one language and one theme, with Shiki's defaults.
    pub fn new(lang: impl Into<String>, theme: impl Into<String>) -> Self {
        Self::with_selection(lang, ThemeSelection::Single(theme.into()))
    }

    /// Options for several themes, like `[("light", "github-light"), ("dark", "github-dark")]`.
    ///
    /// The `light` key is the default color, as in Shiki.
    pub fn with_themes<K, T>(
        lang: impl Into<String>,
        themes: impl IntoIterator<Item = (K, T)>,
    ) -> Self
    where
        K: Into<String>,
        T: Into<String>,
    {
        Self::with_selection(
            lang,
            ThemeSelection::Multiple {
                themes: themes
                    .into_iter()
                    .map(|(k, t)| (k.into(), t.into()))
                    .collect(),
                default_color: DefaultColor::Key("light".into()),
                css_variable_prefix: "--shiki-".into(),
                colors_rendering: ColorsRendering::CssVars,
            },
        )
    }

    fn with_selection(lang: impl Into<String>, theme: ThemeSelection) -> Self {
        Self {
            lang: lang.into(),
            theme,
            color_replacements: HashMap::new(),
            tokenize_max_line_length: 0,
            tokenize_time_limit: Some(Duration::from_millis(500)),
            merge_whitespaces: MergeWhitespaces::Merge,
            merge_same_style_tokens: false,
            tabindex: Some("0".into()),
            root_style: true,
            include_scopes: false,
        }
    }

    /// Sets the default color key of a multi-theme selection.
    pub fn default_color(mut self, value: DefaultColor) -> Self {
        if let ThemeSelection::Multiple { default_color, .. } = &mut self.theme {
            *default_color = value;
        }
        self
    }
}
