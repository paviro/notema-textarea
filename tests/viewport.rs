use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget as _};
use ratatui_textarea::{CursorMove, DataCursor, TextArea, WrapMode};

fn render(textarea: &TextArea<'_>, area: Rect) -> Buffer {
    let mut buffer = Buffer::empty(area);
    textarea.render(area, &mut buffer);
    buffer
}

#[test]
fn padded_mouse_mapping_survives_layout_resize_and_scroll() {
    let source = ["e\u{301}日👩🏽‍💻\tend", "next line", "last"];
    for mode in [WrapMode::None, WrapMode::Glyph, WrapMode::WordOrGlyph] {
        let mut textarea = TextArea::from(source);
        textarea.set_wrap_mode(mode);
        textarea.set_top_padding(2);
        for area in [Rect::new(3, 4, 12, 6), Rect::new(3, 4, 5, 3)] {
            textarea.layout_for(area);
            textarea.move_cursor(CursorMove::Top);
            render(&textarea, area);
            let padding = usize::from(textarea.top_padding());
            assert_eq!(textarea.screen_to_data(0, 0), DataCursor(0, 0));
            assert_eq!(textarea.screen_to_data(padding, 0), DataCursor(0, 0));
            assert_eq!(textarea.screen_to_data(padding, 2), DataCursor(0, 2));

            textarea.move_cursor(CursorMove::Bottom);
            textarea.move_cursor(CursorMove::End);
            render(&textarea, area);
            let screen = textarea.screen_cursor();
            assert_eq!(
                textarea.screen_to_data(screen.row + padding, screen.col),
                textarea.cursor()
            );
            assert_eq!(
                textarea.screen_to_data(usize::MAX, usize::MAX),
                DataCursor(2, 4)
            );
            assert_eq!(textarea.lines(), source);
        }
    }
}

#[test]
fn deleting_scrolled_text_restores_the_start_and_undo_restores_content() {
    let source = "abcdefghijklmnop";
    let mut textarea = TextArea::from([source]);
    let area = Rect::new(0, 0, 6, 1);
    textarea.move_cursor(CursorMove::End);
    render(&textarea, area);
    assert_eq!(textarea.scroll_offset().1, 11);
    for _ in 0..13 {
        assert!(textarea.delete_char());
        render(&textarea, area);
    }
    assert_eq!(textarea.scroll_offset().1, 0);
    assert_eq!(textarea.cursor(), DataCursor(0, 3));
    let buffer = render(&textarea, area);
    assert_eq!(buffer[(0, 0)].symbol(), "a");
    assert_eq!(buffer[(2, 0)].symbol(), "c");
    while textarea.undo() {}
    assert_eq!(textarea.lines(), [source]);
}

#[cfg(feature = "search")]
#[test]
fn matches_before_a_wrapped_fragment_do_not_underflow() {
    let mut textarea = TextArea::from(["find this long wrapped line"]);
    textarea.set_wrap_mode(WrapMode::Glyph);
    textarea.set_search_pattern("find").unwrap();
    textarea.move_cursor(CursorMove::End);
    render(&textarea, Rect::new(0, 0, 5, 6));
    assert_eq!(textarea.lines(), ["find this long wrapped line"]);
}
