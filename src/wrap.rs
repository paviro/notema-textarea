use crate::width::display_width_to;
#[cfg(feature = "arbitrary")]
use arbitrary::Arbitrary;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use unicode_linebreak::{BreakClass, BreakOpportunity, break_property, linebreaks};
use unicode_segmentation::UnicodeSegmentation;

/// Specify how logical lines are soft-wrapped at render time.
///
/// Word modes use Unicode 15.0 line-break opportunities. Dictionary-based
/// segmentation for complex-context scripts is not supported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum WrapMode {
    /// Disable soft wrapping and keep horizontal scrolling behavior.
    None,
    /// Wrap at Unicode line-break opportunities without splitting oversized segments.
    Word,
    /// Wrap at grapheme boundaries.
    Glyph,
    /// Wrap at Unicode line-break opportunities, splitting oversized segments at
    /// grapheme boundaries. Keep trailing punctuation with its preceding grapheme
    /// when that ending fits within the viewport.
    WordOrGlyph,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WrappedLine {
    pub row: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub first_in_row: bool,
    pub last_in_row: bool,
}

pub(crate) fn effective_wrap_width(total_width: u16, line_number_len: Option<u8>) -> usize {
    let total_width = total_width as usize;
    let reserved = line_number_len.map(|len| len as usize + 2).unwrap_or(0);
    if total_width > reserved {
        total_width - reserved
    } else {
        1
    }
}

pub(crate) fn wrapped_rows(
    lines: &[String],
    mode: WrapMode,
    width: usize,
    tab_len: u8,
) -> Vec<WrappedLine> {
    let mut rows = Vec::new();

    for (row, line) in lines.iter().enumerate() {
        let ranges = line_ranges(line, mode, width, tab_len);
        let mut start_col = 0usize;
        for (i, (start_byte, end_byte)) in ranges.iter().copied().enumerate() {
            let end_col = start_col + line[start_byte..end_byte].chars().count();
            rows.push(WrappedLine {
                row,
                start_byte,
                end_byte,
                start_col,
                end_col,
                first_in_row: i == 0,
                last_in_row: i + 1 == ranges.len(),
            });
            start_col = end_col;
        }
    }

    rows
}

pub(crate) fn line_ranges(
    line: &str,
    mode: WrapMode,
    width: usize,
    tab_len: u8,
) -> Vec<(usize, usize)> {
    if mode == WrapMode::None {
        return vec![(0, line.len())];
    }

    let width = width.max(1);
    let mut out = match mode {
        WrapMode::None => vec![(0, line.len())],
        WrapMode::Glyph => {
            let mut chunks = Vec::new();
            split_range_by_grapheme_width(line, 0, line.len(), width, tab_len, &mut chunks);
            chunks
        }
        WrapMode::Word => wrap_word_chunks(line, width, tab_len, false),
        WrapMode::WordOrGlyph => wrap_word_chunks(line, width, tab_len, true),
    };

    if out.is_empty() {
        out.push((0, 0));
    }
    out
}

fn wrap_word_chunks(
    line: &str,
    width: usize,
    tab_len: u8,
    fallback_to_glyph: bool,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut chunk_start = 0;
    let mut row_start = 0;
    let mut row_end = 0;
    let mut row_width = 0;
    let mut grapheme_ends = line
        .grapheme_indices(true)
        .map(|(start, g)| start + g.len())
        .peekable();

    for (end, opportunity) in linebreaks(line) {
        while grapheme_ends.peek().is_some_and(|&boundary| boundary < end) {
            grapheme_ends.next();
        }
        if grapheme_ends.peek() != Some(&end) {
            continue;
        }
        let text = &line[chunk_start..end];
        // Trailing breakable spaces belong to this row even when they overflow.
        // NBSP and other nonbreaking characters still count toward the width.
        let content = text.trim_end_matches(|c| {
            c == '\t'
                || matches!(
                    break_property(c as u32),
                    BreakClass::Space
                        | BreakClass::Mandatory
                        | BreakClass::CarriageReturn
                        | BreakClass::LineFeed
                        | BreakClass::NextLine
                )
        });
        if display_width_to(content, row_width, tab_len) > width && row_end > row_start {
            out.push((row_start, row_end));
            row_start = row_end;
            row_width = 0;
        }

        if fallback_to_glyph && display_width_to(content, 0, tab_len) > width {
            let content_end = chunk_start + content.len();
            split_word_by_grapheme_width(line, chunk_start, content_end, width, tab_len, &mut out);
            // Keep the last fragment open so the next segment can use its space.
            if let Some((start, _)) = out.pop() {
                row_start = start;
                row_width = display_width_to(&line[start..end], 0, tab_len);
            }
        } else {
            row_width = display_width_to(text, row_width, tab_len);
        }
        row_end = end;
        chunk_start = end;

        if opportunity == BreakOpportunity::Mandatory {
            out.push((row_start, row_end));
            row_start = row_end;
            row_width = 0;
        }
    }

    if row_end > row_start {
        out.push((row_start, row_end));
    }
    out
}

fn split_word_by_grapheme_width(
    line: &str,
    start: usize,
    end: usize,
    width: usize,
    tab_len: u8,
    out: &mut Vec<(usize, usize)>,
) {
    let mut tail_start = end;
    for (offset, grapheme) in line[start..end].grapheme_indices(true).rev() {
        if grapheme.chars().next().is_some_and(is_trailing_punctuation) {
            tail_start = start + offset;
        } else {
            if tail_start < end && !grapheme.chars().all(char::is_whitespace) {
                tail_start = start + offset;
            }
            break;
        }
    }

    if tail_start == end || display_width_to(&line[tail_start..end], 0, tab_len) > width {
        split_range_by_grapheme_width(line, start, end, width, tab_len, out);
        return;
    }

    let first_fragment = out.len();
    split_range_by_grapheme_width(line, start, tail_start, width, tab_len, out);
    if out.len() > first_fragment {
        let last = out.last_mut().expect("the prefix produced a fragment");
        if display_width_to(&line[last.0..end], 0, tab_len) <= width {
            last.1 = end;
            return;
        }
    }
    out.push((tail_start, end));
}

fn is_trailing_punctuation(c: char) -> bool {
    matches!(
        break_property(c as u32),
        BreakClass::ClosePunctuation
            | BreakClass::CloseParenthesis
            | BreakClass::Exclamation
            | BreakClass::InfixSeparator
            | BreakClass::Inseparable
            | BreakClass::NonStarter
            | BreakClass::Quotation
            | BreakClass::Postfix
            | BreakClass::Symbol
    )
}

fn split_range_by_grapheme_width(
    line: &str,
    start: usize,
    end: usize,
    width: usize,
    tab_len: u8,
    out: &mut Vec<(usize, usize)>,
) {
    let mut segment_start = start;
    while segment_start < end {
        let mut segment_end = segment_start;
        let mut segment_width = 0usize;

        for (offset, grapheme) in
            UnicodeSegmentation::grapheme_indices(&line[segment_start..end], true)
        {
            let grapheme_start = segment_start + offset;
            let grapheme_end = grapheme_start + grapheme.len();
            let next_width = display_width_to(grapheme, segment_width, tab_len);
            let grapheme_width = next_width.saturating_sub(segment_width);

            if segment_end != segment_start && segment_width + grapheme_width > width {
                break;
            }

            segment_end = grapheme_end;
            segment_width = next_width;
            if segment_width > width {
                break;
            }
        }

        if segment_end == segment_start {
            if let Some(ch) = line[segment_start..end].chars().next() {
                segment_end = segment_start + ch.len_utf8();
            } else {
                break;
            }
        }

        out.push((segment_start, segment_end));
        segment_start = segment_end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(line: &str, mode: WrapMode, width: usize) -> Vec<&str> {
        line_ranges(line, mode, width, 4)
            .into_iter()
            .map(|(s, e)| &line[s..e])
            .collect()
    }

    #[test]
    fn emoji_width_matches_its_rendered_cells() {
        for glyph in ["👩🏽‍💻", "🇩🇪", "❤️", "1️⃣"] {
            for mode in [WrapMode::Glyph, WrapMode::WordOrGlyph] {
                let text = format!("a{glyph}b{glyph}c");
                assert_eq!(
                    segments(&text, mode, 4),
                    [format!("a{glyph}b"), format!("{glyph}c")]
                );
            }
        }
    }

    #[test]
    fn word_wrap_keeps_long_word() {
        let have = segments("helloworld", WrapMode::Word, 4);
        assert_eq!(have, vec!["helloworld"]);
    }

    #[test]
    fn word_or_glyph_wrap_splits_long_word() {
        let have = segments("helloworld", WrapMode::WordOrGlyph, 4);
        assert_eq!(have, vec!["hell", "owor", "ld"]);
    }

    #[test]
    fn word_wrap_keeps_space_off_continuation_start() {
        let have = segments("Hello this is a test", WrapMode::WordOrGlyph, 15);
        assert_eq!(have, vec!["Hello this is a ", "test"]);
        assert!(have[1..].iter().all(|s| !s.starts_with(' ')));
    }

    #[test]
    fn unicode_wrap_keeps_punctuation_with_the_word() {
        for mode in [WrapMode::Word, WrapMode::WordOrGlyph] {
            for punctuation in [
                ",", ".", "!", "?", ":", ";", "…", "?!", "...", ")", "]", "}", "\"", "”", "’",
                "。」",
            ] {
                let text = format!("a word{punctuation} next");
                let width = display_width_to(&format!("word{punctuation}"), 0, 4).max(6);
                let rows = segments(&text, mode, width);
                assert_eq!(rows[0], "a ", "{mode:?}: {text}");
                assert_eq!(rows[1], format!("word{punctuation} "), "{mode:?}: {text}");
                assert_eq!(rows.concat(), text);
            }
        }
    }

    #[test]
    fn unicode_wrap_respects_opening_punctuation_and_cjk_breaks() {
        for mode in [WrapMode::Word, WrapMode::WordOrGlyph] {
            assert_eq!(segments("a (word) end", mode, 7), ["a ", "(word) ", "end"]);
            assert_eq!(segments("你好，世界。", mode, 6), ["你好，", "世界。"]);
            assert_eq!(
                segments("你好，世界。", mode, 4),
                ["你", "好，", "世", "界。"]
            );
        }
    }

    #[test]
    fn unicode_wrap_respects_nonbreaking_and_zero_width_characters() {
        for mode in [WrapMode::Word, WrapMode::WordOrGlyph] {
            for joined in ["ab\u{a0}cd", "ab\u{202f}cd", "ab\u{2060}cd"] {
                let text = format!("x {joined}");
                assert_eq!(segments(&text, mode, 5), ["x ", joined]);
            }
            assert_eq!(segments("ab\u{200b}cd", mode, 2), ["ab\u{200b}", "cd"]);
        }
    }

    #[test]
    fn unicode_wrap_honors_mandatory_breaks_without_extra_end_row() {
        for mode in [WrapMode::Word, WrapMode::WordOrGlyph] {
            for separator in ["\n", "\r\n", "\u{85}", "\u{2028}", "\u{2029}"] {
                let text = format!("one{separator}two{separator}");
                let rows = segments(&text, mode, 80);
                assert_eq!(rows, [format!("one{separator}"), format!("two{separator}")]);
                assert_eq!(rows.concat(), text);
            }
        }
    }

    #[test]
    fn unicode_wrap_keeps_overflowing_spaces_on_the_previous_row() {
        for mode in [WrapMode::Word, WrapMode::WordOrGlyph] {
            assert_eq!(segments("hello   world", mode, 5), ["hello   ", "world"]);
            assert_eq!(segments("hello\tworld", mode, 5), ["hello\t", "world"]);
            assert_eq!(segments("ab\tcd ef", mode, 6), ["ab\tcd ", "ef"]);
            assert_eq!(segments("     ", mode, 2), ["     "]);
            assert_eq!(segments("", mode, 2), [""]);
        }
    }

    #[test]
    fn emergency_wrap_keeps_the_punctuated_ending_together() {
        for (text, width, expected) in [
            ("hello,", 5, vec!["hell", "o,"]),
            ("abcdef?!", 4, vec!["abcd", "ef?!"]),
            ("abcdef?!", 3, vec!["abc", "de", "f?!"]),
            ("abcdef?! next", 6, vec!["abcde", "f?! ", "next"]),
            ("hello, next", 8, vec!["hello, ", "next"]),
            ("abcde\u{301}!", 5, vec!["abcd", "e\u{301}!"]),
            ("hello,\u{2028}next", 5, vec!["hell", "o,\u{2028}", "next"]),
            ("hello,", 1, vec!["h", "e", "l", "l", "o", ","]),
            ("abc?!", 2, vec!["ab", "c?", "!"]),
        ] {
            assert_eq!(
                segments(text, WrapMode::WordOrGlyph, width),
                expected,
                "{text} at {width}"
            );
        }
    }

    #[test]
    fn wrapped_ranges_cover_the_source_and_preserve_graphemes() {
        for text in [
            "hello, world?!",
            "你好，世界。",
            "a e\u{301}! 👩🏽‍💻 🇩🇪 end",
            "a\tword...   next",
            "ab\u{a0}cd\u{2060}ef",
            "one\u{2028}two\r\nthree",
        ] {
            let boundaries: Vec<_> = text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([text.len()])
                .collect();
            for mode in [WrapMode::Word, WrapMode::WordOrGlyph, WrapMode::Glyph] {
                for width in 0..=20 {
                    let rows = line_ranges(text, mode, width, 4);
                    let mut previous_end = 0;
                    for (start, end) in rows {
                        assert_eq!(start, previous_end, "{text:?} {mode:?} {width}");
                        assert!(end > start);
                        assert!(boundaries.contains(&end), "{text:?} at {end}");
                        previous_end = end;
                    }
                    assert_eq!(previous_end, text.len());
                }
            }
        }
    }

    #[test]
    fn glyph_wrap_handles_wide_chars() {
        let have = segments("ab犬猫", WrapMode::Glyph, 4);
        assert_eq!(have, vec!["ab犬", "猫"]);
    }

    #[test]
    fn glyph_wrap_keeps_combining_grapheme_cluster() {
        let have = segments("e\u{301}x", WrapMode::Glyph, 1);
        assert_eq!(have, vec!["e\u{301}", "x"]);
    }

    #[test]
    fn tab_width_is_accounted_for_in_wrap() {
        let have = segments("\tX", WrapMode::WordOrGlyph, 2);
        assert_eq!(have, vec!["\t", "X"]);
    }

    #[test]
    fn glyph_wrap_preserves_full_mixed_width_row_capacity() {
        let have = segments("a中bcde", WrapMode::Glyph, 4);
        assert_eq!(have, vec!["a中b", "cde"]);
    }
}
