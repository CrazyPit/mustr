/// Formats an age in seconds as a compact human phrase like `2 days ago`.
///
/// Buckets: under a minute is `just now`; then minutes, hours, days, weeks. A
/// negative age (clock skew) is treated as `just now`.
pub fn humanize_age(seconds: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const WEEK: i64 = 7 * DAY;

    let unit = |n: i64, singular: &str| {
        if n == 1 {
            format!("1 {singular} ago")
        } else {
            format!("{n} {singular}s ago")
        }
    };

    match seconds {
        s if s < MINUTE => "just now".to_string(),
        s if s < HOUR => unit(s / MINUTE, "minute"),
        s if s < DAY => unit(s / HOUR, "hour"),
        s if s < WEEK => unit(s / DAY, "day"),
        s => unit(s / WEEK, "week"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_a_minute_is_just_now() {
        assert_eq!(humanize_age(0), "just now");
        assert_eq!(humanize_age(59), "just now");
    }

    #[test]
    fn negative_age_is_just_now() {
        assert_eq!(humanize_age(-5), "just now");
    }

    #[test]
    fn minutes_singular_and_plural() {
        assert_eq!(humanize_age(60), "1 minute ago");
        assert_eq!(humanize_age(3599), "59 minutes ago");
    }

    #[test]
    fn hours_singular_and_plural() {
        assert_eq!(humanize_age(3600), "1 hour ago");
        assert_eq!(humanize_age(7200), "2 hours ago");
    }

    #[test]
    fn days_singular_and_plural() {
        assert_eq!(humanize_age(86_400), "1 day ago");
        assert_eq!(humanize_age(2 * 86_400), "2 days ago");
    }

    #[test]
    fn weeks_for_a_week_or_more() {
        assert_eq!(humanize_age(7 * 86_400), "1 week ago");
        assert_eq!(humanize_age(3 * 7 * 86_400), "3 weeks ago");
    }
}
