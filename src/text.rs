use unicode_segmentation::UnicodeSegmentation;

const ZWJ: char = '\u{200D}';

/// Make mesh strings safe to draw in a terminal that draws each codepoint on
/// its own. Strips control, replacement and invisible chars, and reduces each
/// emoji sequence to one char, because terminals disagree on the width of a
/// sequence: a flag becomes its country code, a ZWJ sequence its last emoji,
/// and other sequences their base emoji.
pub fn sanitize(s: &str) -> String {
    let mut text = String::new();
    for cluster in s.graphemes(true) {
        if cluster.chars().all(is_regional_indicator) {
            text.extend(cluster.chars().map(country_letter));
            continue;
        }
        let shown = if cluster.starts_with(is_pictograph) {
            cluster
                .rsplit(ZWJ)
                .find(|part| !part.is_empty())
                .unwrap_or(cluster)
        } else {
            cluster
        };
        text.extend(shown.chars().filter(|c| is_drawable(*c)));
    }
    text.trim().to_string()
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// The letter a regional indicator stands for; a pair of them is a flag
fn country_letter(c: char) -> char {
    char::from(b'A' + (c as u32 - 0x1F1E6) as u8)
}

/// Blocks that emoji ZWJ sequences start in; a rough stand-in for
/// Extended_Pictographic that leaves ZWJs in other scripts' clusters alone
fn is_pictograph(c: char) -> bool {
    matches!(c, '\u{2600}'..='\u{27BF}' | '\u{2B00}'..='\u{2BFF}' | '\u{1F000}'..='\u{1FAFF}')
}

/// False for chars that draw nothing or change how their neighbours draw
fn is_drawable(c: char) -> bool {
    !(c == '\u{FFFD}'
        || c.is_control()
        || matches!(c,
            // Zero-width chars, including ZWJ, and invisible separators
            '\u{200B}'..='\u{200F}' | '\u{2028}' | '\u{2029}' | '\u{2060}' | '\u{FEFF}' |
            // Bidi embeddings, overrides and isolates
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' |
            // Emoji sequence parts: presentation selectors, skin tones, keycap, tags
            '\u{FE0E}' | '\u{FE0F}' | '\u{1F3FB}'..='\u{1F3FF}' | '\u{20E3}' | '\u{E0020}'..='\u{E007F}'
        ))
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

/// Every unit `format_duration` has, so it keeps them all
pub const EVERY_UNIT: usize = 5;

/// Format seconds as weeks, days, hours, minutes and seconds, keeping `units`
/// of them from the largest non-zero one and leaving out zero units (e.g.
/// "1d4h32s", or "1d4h" with 2 units)
pub fn format_duration(secs: u32, units: usize) -> String {
    let places = [
        (secs / 604_800, "w"),
        (secs / 86_400 % 7, "d"),
        (secs / 3_600 % 24, "h"),
        (secs / 60 % 60, "m"),
        (secs % 60, "s"),
    ];
    let text: String = places
        .iter()
        .skip_while(|(n, _)| *n == 0)
        .take(units)
        .filter(|(n, _)| *n > 0)
        .map(|(n, unit)| format!("{}{}", n, unit))
        .collect();
    if text.is_empty() {
        "0s".to_string()
    } else {
        text
    }
}

/// Decode a URL query value: "+" is a space and "%XX" a byte. None when a "%"
/// isn't followed by two hex digits or the bytes aren't UTF-8.
pub fn url_decode(s: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut rest = s.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        rest = tail;
        match b {
            b'+' => bytes.push(b' '),
            b'%' => {
                let hex = std::str::from_utf8(rest.get(..2)?).ok()?;
                bytes.push(u8::from_str_radix(hex, 16).ok()?);
                rest = &rest[2..];
            }
            _ => bytes.push(b),
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_decode_turns_plus_and_percent_escapes_into_text() {
        assert_eq!(
            url_decode("Example+Contact").as_deref(),
            Some("Example Contact")
        );
        assert_eq!(
            url_decode("Hilltop%20%F0%9F%96%96").as_deref(),
            Some("Hilltop 🖖")
        );
        assert_eq!(url_decode("100%25").as_deref(), Some("100%"));
        assert_eq!(url_decode("bad%2"), None);
        assert_eq!(url_decode("bad%zz"), None);
    }

    #[test]
    fn durations_use_the_largest_units_and_skip_zeros() {
        assert_eq!(format_duration(0, EVERY_UNIT), "0s");
        assert_eq!(format_duration(4, EVERY_UNIT), "4s");
        assert_eq!(format_duration(2973, EVERY_UNIT), "49m33s");
        assert_eq!(format_duration(3600, EVERY_UNIT), "1h");
        assert_eq!(
            format_duration(86_400 + 4 * 3_600 + 32, EVERY_UNIT),
            "1d4h32s"
        );
        assert_eq!(format_duration(9 * 86_400 + 60, EVERY_UNIT), "1w2d1m");
    }

    #[test]
    fn durations_keep_the_given_number_of_units_from_the_largest() {
        assert_eq!(format_duration(2973, 2), "49m33s");
        assert_eq!(format_duration(18 * 3_600 + 29 * 60 + 2, 2), "18h29m");
        assert_eq!(format_duration(875 * 86_400 + 18 * 3_600, 2), "125w");
        assert_eq!(format_duration(874 * 86_400 + 29 * 60, 2), "124w6d");
        // A zero unit inside the kept ones is left out, not replaced by a smaller one
        assert_eq!(format_duration(86_400 + 5 * 60, 2), "1d");
        assert_eq!(format_duration(86_400 + 4 * 3_600 + 32, 3), "1d4h");
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
    fn sanitize_keeps_single_emoji_symbols_and_cjk() {
        assert_eq!(sanitize("hi 😀 ☀ 日本 한국"), "hi 😀 ☀ 日本 한국");
    }

    #[test]
    fn sanitize_keeps_combining_accents() {
        assert_eq!(sanitize("Zoe\u{0308}"), "Zoe\u{0308}");
    }

    #[test]
    fn sanitize_turns_flags_into_country_codes() {
        assert_eq!(sanitize("🇩🇪 Berlin"), "DE Berlin");
        assert_eq!(sanitize("🇺🇸🇫🇷"), "USFR");
    }

    #[test]
    fn sanitize_reduces_zwj_sequences_to_their_last_emoji() {
        assert_eq!(sanitize("🏳️‍⚧️ 🏳️‍🌈"), "⚧ 🌈");
        assert_eq!(sanitize("👩‍💻"), "💻");
    }

    #[test]
    fn sanitize_reduces_other_emoji_sequences_to_their_base() {
        assert_eq!(sanitize("👍🏽 ☀️ 1️⃣"), "👍 ☀ 1");
        assert_eq!(
            sanitize("🏴\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}"),
            "🏴"
        );
    }

    #[test]
    fn sanitize_strips_invisible_and_bidi_chars() {
        assert_eq!(
            sanitize("a\u{200B}b\u{202E}c\u{2066}d\u{FEFF}e\u{2028}f\u{2060}g\u{200D}h"),
            "abcdefgh"
        );
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
