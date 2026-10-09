use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn grapheme_width(grapheme: &str, column: usize, tab_len: u8) -> usize {
    if grapheme == "\t" {
        if tab_len == 0 {
            0
        } else {
            let tab = usize::from(tab_len);
            tab - column % tab
        }
    } else if grapheme.chars().all(char::is_control) {
        0
    } else {
        grapheme.width()
    }
}

pub(crate) fn display_width_to(text: &str, mut column: usize, tab_len: u8) -> usize {
    for grapheme in text.graphemes(true) {
        column += grapheme_width(grapheme, column, tab_len);
    }
    column
}
