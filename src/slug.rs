/// Maximum length of a generated slug, in characters. Long enough to stay
/// readable, short enough to keep paths sane.
const MAX_SLUG_LEN: usize = 64;

/// Turns a human name into a filesystem-safe slug.
///
/// Lowercases, collapses every run of non-alphanumeric characters into a single
/// `-`, trims leading/trailing `-`, truncates to [`MAX_SLUG_LEN`] characters, then
/// re-trims edges so truncation can never leave a dangling `-`. Returns an empty
/// string when the name has no alphanumeric content; callers reject that.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            slug.extend(ch.to_lowercase());
            prev_sep = false;
        } else if !slug.is_empty() && !prev_sep {
            slug.push('-');
            prev_sep = true;
        }
    }

    let mut slug: String = slug.chars().take(MAX_SLUG_LEN).collect();
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_and_hyphenates_spaces() {
        assert_eq!(slugify("Fix Login"), "fix-login");
    }

    #[test]
    fn collapses_whitespace_and_trims_edges() {
        assert_eq!(slugify("  Hello   World  "), "hello-world");
    }

    #[test]
    fn collapses_punctuation_runs_to_single_hyphen() {
        assert_eq!(slugify("foo/bar baz!!"), "foo-bar-baz");
    }

    #[test]
    fn trims_leading_and_trailing_separators() {
        assert_eq!(slugify("---trim---"), "trim");
    }

    #[test]
    fn keeps_unicode_letters() {
        assert_eq!(slugify("Привет Мир"), "привет-мир");
    }

    #[test]
    fn empty_name_yields_empty_slug() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn punctuation_only_yields_empty_slug() {
        assert_eq!(slugify("!!! ???"), "");
    }

    #[test]
    fn truncates_to_max_length_without_trailing_hyphen() {
        // 63 x's, then a separator run, then more text. After collapsing this is
        // "xxx…(63)-yy"; truncating to 64 chars lands on the hyphen, which the
        // re-trim must strip.
        let name = format!("{}  yy", "x".repeat(63));
        let slug = slugify(&name);
        assert_eq!(slug, "x".repeat(63));
        assert!(slug.chars().count() <= MAX_SLUG_LEN);
        assert!(!slug.ends_with('-'));
    }
}
