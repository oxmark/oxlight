//! HTML rendering, ported from Shiki's `tokensToHast` and the escaping rules
//! of `hast-util-to-html`.

use crate::options::HighlightOptions;
use crate::tokens::TokensResult;

/// Escapes text content: `&` and `<`, as hexadecimal references.
pub(crate) fn escape_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&#x26;"),
            '<' => out.push_str("&#x3C;"),
            _ => out.push(ch),
        }
    }
}

/// Escapes a double quoted attribute value.
pub(crate) fn escape_attribute(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&#x26;"),
            '"' => out.push_str("&#x22;"),
            '\'' => out.push_str("&#x27;"),
            '`' => out.push_str("&#x60;"),
            _ => out.push(ch),
        }
    }
}

fn attribute(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape_attribute(value, out);
    out.push('"');
}

/// Renders tokens to the `<pre class="shiki">` markup of Shiki.
pub(crate) fn tokens_to_html(result: &TokensResult, options: &HighlightOptions) -> String {
    let mut out = String::with_capacity(
        result
            .tokens
            .iter()
            .map(|l| l.iter().map(|t| t.content.len() + 32).sum::<usize>() + 24)
            .sum::<usize>()
            + 128,
    );
    out.push_str("<pre");
    attribute(&mut out, "class", &format!("shiki {}", result.theme_name));
    if options.root_style {
        let style = match &result.root_style {
            Some(style) => style.clone(),
            None => format!("background-color:{};color:{}", result.bg, result.fg),
        };
        attribute(&mut out, "style", &style);
    }
    if let Some(tabindex) = &options.tabindex {
        attribute(&mut out, "tabindex", tabindex);
    }
    out.push_str("><code>");
    for (idx, line) in result.tokens.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str("<span class=\"line\">");
        for token in line {
            out.push_str("<span");
            let style = token.style_string();
            if !style.is_empty() {
                attribute(&mut out, "style", &style);
            }
            out.push('>');
            escape_text(&token.content, &mut out);
            out.push_str("</span>");
        }
        out.push_str("</span>");
    }
    out.push_str("</code></pre>");
    out
}
