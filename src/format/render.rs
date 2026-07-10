//! tmux format-string rendering: substitute `#{var}` from a [`FormatContext`].
//!
//! Deliberately not a full format engine (no conditionals or arithmetic) — the
//! agent tools we target only use plain `#{var}` substitution. Unresolved
//! variables are stripped, matching the shim's compatibility contract.

use super::context::FormatContext;

#[must_use]
pub fn render(format: &str, ctx: &FormatContext) -> String {
    let mut out = String::with_capacity(format.len());
    let mut rest = format;
    while let Some(pos) = rest.find("#{") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        if let Some(end) = after.find('}') {
            if let Some(value) = ctx.lookup(&after[..end]) {
                out.push_str(value);
            }
            rest = &after[end + 1..];
        } else {
            out.push_str("#{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn ctx() -> FormatContext {
        FormatContext::test_ctx(&[
            ("pane_id", "%3"),
            ("pane_width", "80"),
            ("pane_height", "24"),
            ("window_width", "80"),
            ("empty_var", ""),
        ])
    }

    #[test]
    fn single_variable() {
        assert_eq!(render("#{pane_width}", &ctx()), "80");
    }

    #[test]
    fn multiple_variables_with_literals() {
        assert_eq!(render("#{pane_width}x#{pane_height}", &ctx()), "80x24");
        assert_eq!(render("#{pane_width},#{window_width}", &ctx()), "80,80");
        assert_eq!(render("id=#{pane_id}!", &ctx()), "id=%3!");
    }

    #[test]
    fn unknown_variable_is_stripped() {
        assert_eq!(render("a#{nope}b", &ctx()), "ab");
    }

    #[test]
    fn known_empty_variable_renders_empty() {
        assert_eq!(render("[#{empty_var}]", &ctx()), "[]");
    }

    #[test]
    fn unterminated_brace_is_literal() {
        assert_eq!(render("#{unclosed", &ctx()), "#{unclosed");
    }

    #[test]
    fn plain_text_passthrough() {
        assert_eq!(render("no vars here", &ctx()), "no vars here");
    }
}
