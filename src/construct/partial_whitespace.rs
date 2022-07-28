//! Trailing whitespace occurs in [string][] and [text][].
//!
//! It occurs around line endings, and, in the case of text content it also
//! occurs at the start or end of the whole.
//!
//! They’re formed with the following BNF:
//!
//! ```bnf
//! ; Restriction: the start and end here count as an eol in the case of `text`.
//! whitespace ::= 0.*space_or_tab eol 0.*space_or_tab
//! ```
//!
//! Normally this whitespace is ignored.
//! In the case of text content, whitespace before a line ending that
//! consistents solely of spaces, at least 2, forms a hard break (trailing).
//!
//! The minimum number of the spaces is defined in
//! [`HARD_BREAK_PREFIX_SIZE_MIN`][hard_break_prefix_size_min].
//!
//! Hard breaks in markdown relate to the HTML element `<br>`.
//! See [*§ 4.5.27 The `br` element* in the HTML spec][html] for more info.
//!
//! It is also possible to create a hard break with a similar construct: a
//! [hard break (escape)][hard_break_escape] is a backslash followed
//! by a line ending.
//! That construct is recommended because it is similar to a
//! [character escape][character_escape] and similar to how line endings can be
//! “escaped” in other languages.
//! Trailing spaces are typically invisible in editors, or even automatically
//! removed, making hard break (trailing) hard to use.
//! ## Tokens
//!
//! *   [`HardBreakTrailing`][Token::HardBreakTrailing]
//! *   [`SpaceOrTab`][Token::SpaceOrTab]
//!
//! ## References
//!
//! *   [`initialize/text.js` in `micromark`](https://github.com/micromark/micromark/blob/main/packages/micromark/dev/lib/initialize/text.js)
//! *   [*§ 6.7 Hard line breaks* in `CommonMark`](https://spec.commonmark.org/0.30/#hard-line-breaks)
//!
//! [string]: crate::content::string
//! [text]: crate::content::text
//! [hard_break_escape]: crate::construct::hard_break_escape
//! [character_escape]: crate::construct::character_escape
//! [hard_break_prefix_size_min]: crate::constant::HARD_BREAK_PREFIX_SIZE_MIN
//! [html]: https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element

use crate::constant::HARD_BREAK_PREFIX_SIZE_MIN;
use crate::token::Token;
use crate::tokenizer::{Event, EventType, Tokenizer};
use crate::util::slice::{Position, Slice};

/// To do.
pub fn create_resolve_whitespace(hard_break: bool, trim_whole: bool) -> impl Fn(&mut Tokenizer) {
    move |t| resolve_whitespace(t, hard_break, trim_whole)
}

/// To do.
pub fn resolve_whitespace(tokenizer: &mut Tokenizer, hard_break: bool, trim_whole: bool) {
    let mut index = 0;

    while index < tokenizer.events.len() {
        let event = &tokenizer.events[index];

        if event.event_type == EventType::Exit && event.token_type == Token::Data {
            let trim_start = (trim_whole && index == 1)
                || (index > 1 && tokenizer.events[index - 2].token_type == Token::LineEnding);
            let trim_end = (trim_whole && index == tokenizer.events.len() - 1)
                || (index + 1 < tokenizer.events.len()
                    && tokenizer.events[index + 1].token_type == Token::LineEnding);

            trim_data(tokenizer, index, trim_start, trim_end, hard_break);
        }

        index += 1;
    }
}

/// To do.
#[allow(clippy::too_many_lines)]
fn trim_data(
    tokenizer: &mut Tokenizer,
    exit_index: usize,
    trim_start: bool,
    trim_end: bool,
    hard_break: bool,
) {
    let mut slice = Slice::from_position(
        &tokenizer.parse_state.chars,
        &Position::from_exit_event(&tokenizer.events, exit_index),
    );

    if trim_end {
        let mut index = slice.chars.len();
        let vs = slice.after;
        let mut spaces_only = vs == 0;
        while index > 0 {
            match slice.chars[index - 1] {
                ' ' => {}
                '\t' => spaces_only = false,
                _ => break,
            }

            index -= 1;
        }

        let diff = slice.chars.len() - index;
        let token_type = if spaces_only
            && hard_break
            && exit_index + 1 < tokenizer.events.len()
            && diff >= HARD_BREAK_PREFIX_SIZE_MIN
        {
            Token::HardBreakTrailing
        } else {
            Token::SpaceOrTab
        };

        // The whole data is whitespace.
        // We can be very fast: we only change the token types.
        if index == 0 {
            tokenizer.events[exit_index - 1].token_type = token_type.clone();
            tokenizer.events[exit_index].token_type = token_type;
            return;
        }

        if diff > 0 || vs > 0 {
            let exit_point = tokenizer.events[exit_index].point.clone();
            let mut enter_point = exit_point.clone();
            enter_point.index -= diff;
            enter_point.column -= diff;
            enter_point.vs = 0;

            tokenizer.map.add(
                exit_index + 1,
                0,
                vec![
                    Event {
                        event_type: EventType::Enter,
                        token_type: token_type.clone(),
                        point: enter_point.clone(),
                        link: None,
                    },
                    Event {
                        event_type: EventType::Exit,
                        token_type,
                        point: exit_point,
                        link: None,
                    },
                ],
            );

            tokenizer.events[exit_index].point = enter_point;
            slice.chars = &slice.chars[..index];
        }
    }

    if trim_start {
        let mut index = 0;
        let vs = slice.before;
        while index < slice.chars.len() {
            match slice.chars[index] {
                ' ' | '\t' => {}
                _ => break,
            }

            index += 1;
        }

        // The whole data is whitespace.
        // We can be very fast: we only change the token types.
        if index == slice.chars.len() {
            tokenizer.events[exit_index - 1].token_type = Token::SpaceOrTab;
            tokenizer.events[exit_index].token_type = Token::SpaceOrTab;
            return;
        }

        if index > 0 || vs > 0 {
            let enter_point = tokenizer.events[exit_index - 1].point.clone();
            let mut exit_point = enter_point.clone();
            exit_point.index += index;
            exit_point.column += index;
            exit_point.vs = 0;

            tokenizer.map.add(
                exit_index - 1,
                0,
                vec![
                    Event {
                        event_type: EventType::Enter,
                        token_type: Token::SpaceOrTab,
                        point: enter_point,
                        link: None,
                    },
                    Event {
                        event_type: EventType::Exit,
                        token_type: Token::SpaceOrTab,
                        point: exit_point.clone(),
                        link: None,
                    },
                ],
            );

            tokenizer.events[exit_index - 1].point = exit_point;
        }
    }
}
