use surtr_analysis::{LineIndex, TextPosition, Utf16Position};

#[test]
fn line_index_maps_byte_offsets_to_text_and_utf16_positions() {
    let source = "a😀\nb";
    let index = LineIndex::new(source);

    assert_eq!(
        index.byte_to_text_position(0),
        TextPosition {
            line: 0,
            character: 0
        }
    );
    assert_eq!(
        index.byte_to_text_position("a".len()),
        TextPosition {
            line: 0,
            character: 1
        }
    );
    assert_eq!(
        index.byte_to_text_position("a😀".len()),
        TextPosition {
            line: 0,
            character: 2
        }
    );
    assert_eq!(
        index.byte_to_text_position("a😀\n".len()),
        TextPosition {
            line: 1,
            character: 0
        }
    );

    assert_eq!(
        index.byte_to_utf16_position("a😀".len()),
        Utf16Position {
            line: 0,
            character: 3
        }
    );
    assert_eq!(
        index.utf16_position_to_byte(Utf16Position {
            line: 0,
            character: 3
        }),
        Some("a😀".len())
    );
    assert_eq!(
        index.utf16_position_to_byte(Utf16Position {
            line: 0,
            character: 2
        }),
        None
    );
}

#[test]
fn line_index_clamps_byte_offsets_inside_codepoints_to_the_previous_boundary() {
    let source = "a😀";
    let index = LineIndex::new(source);

    assert_eq!(
        index.byte_to_text_position(2),
        TextPosition {
            line: 0,
            character: 1
        }
    );
}
