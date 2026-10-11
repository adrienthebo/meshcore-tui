/// Strip Unicode replacement chars, control chars, and emoji from mesh strings.
/// Emoji have inconsistent terminal widths that break TUI border alignment.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| {
            if *c == '\u{FFFD}' || (*c != ' ' && c.is_control()) {
                return false;
            }
            // Strip emoji and other wide/variable-width Unicode blocks
            let cp = *c as u32;
            // Keep ASCII and basic Latin/extended chars
            if cp < 0x2600 {
                return true;
            }
            // Block various emoji and symbol ranges
            matches!(cp,
                // Allow box drawing, block elements, etc. used by TUI
                0x2500..=0x257F | // Box Drawing
                0x2580..=0x259F   // Block Elements
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Split text into chunks of at most `width` display columns; a char wider
/// than `width` gets a chunk of its own so wrapping always makes progress
pub fn wrap_to_width(text: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    let mut w = 0usize;
    for c in text.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if w + cw > width && !chunk.is_empty() {
            chunks.push(std::mem::take(&mut chunk));
            w = 0;
        }
        chunk.push(c);
        w += cw;
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

/// Truncate a string to fit within `max_width` display columns
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut w = 0usize;
    let mut result = String::new();
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if w + cw > max_width {
            break;
        }
        w += cw;
        result.push(c);
    }
    result
}

/// Format seconds as days, hours, minutes and seconds, leaving out zero units (e.g. "1d4h32s")
pub fn format_duration(secs: u32) -> String {
    let units = [(secs / 86_400, "d"), (secs / 3_600 % 24, "h"), (secs / 60 % 60, "m"), (secs % 60, "s")];
    let text: String = units
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, unit)| format!("{}{}", n, unit))
        .collect();
    if text.is_empty() {
        "0s".to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_use_the_largest_units_and_skip_zeros() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(4), "4s");
        assert_eq!(format_duration(2973), "49m33s");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(86_400 + 4 * 3_600 + 32), "1d4h32s");
    }

    // ── sanitize ─────────────────────────────────────────────────────────────

    #[test]
    fn sanitize_keeps_plain_and_latin_text() {
        assert_eq!(sanitize("hello world"), "hello world");
        assert_eq!(sanitize("Zoë Ångström"), "Zoë Ångström");
    }

    #[test]
    fn sanitize_strips_control_and_replacement_chars() {
        assert_eq!(sanitize("a\u{0}b\tc\u{FFFD}d"), "abcd");
    }

    #[test]
    fn sanitize_strips_emoji_and_symbols_at_or_above_u2600() {
        assert_eq!(sanitize("hi 😀 ☀ there"), "hi   there");
    }

    #[test]
    fn sanitize_strips_cjk() {
        assert_eq!(sanitize("node 日本"), "node");
    }

    #[test]
    fn sanitize_keeps_box_drawing() {
        assert_eq!(sanitize("─│▀"), "─│▀");
    }

    #[test]
    fn sanitize_trims_surrounding_whitespace() {
        assert_eq!(sanitize("  name \n"), "name");
    }

    // ── truncate_to_width ────────────────────────────────────────────────────

    #[test]
    fn truncate_shorter_than_width_is_unchanged() {
        assert_eq!(truncate_to_width("abc", 10), "abc");
    }

    #[test]
    fn truncate_cuts_at_width() {
        assert_eq!(truncate_to_width("abcdef", 4), "abcd");
        assert_eq!(truncate_to_width("abc", 0), "");
    }

    #[test]
    fn truncate_counts_display_columns_not_chars() {
        // Each CJK char is two columns wide; a third would exceed 5 columns.
        assert_eq!(truncate_to_width("日本語", 5), "日本");
    }

    // ── wrap_to_width ────────────────────────────────────────────────────────

    #[test]
    fn wrap_short_text_is_one_chunk() {
        assert_eq!(wrap_to_width("abc", 10), vec!["abc"]);
    }

    #[test]
    fn wrap_splits_at_width() {
        assert_eq!(wrap_to_width("abcdefg", 3), vec!["abc", "def", "g"]);
    }

    #[test]
    fn wrap_counts_display_columns() {
        assert_eq!(wrap_to_width("日本語x", 4), vec!["日本", "語x"]);
    }

    #[test]
    fn wrap_char_wider_than_width_gets_own_chunk() {
        assert_eq!(wrap_to_width("a日b", 1), vec!["a", "日", "b"]);
    }

    #[test]
    fn wrap_zero_width_still_makes_progress() {
        assert_eq!(wrap_to_width("ab", 0), vec!["a", "b"]);
    }

    #[test]
    fn wrap_empty_text_has_no_chunks() {
        assert!(wrap_to_width("", 5).is_empty());
    }
}
