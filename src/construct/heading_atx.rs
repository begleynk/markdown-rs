//! Heading (atx) is a construct that occurs in the [flow] content type.
//!
//! They’re formed with the following BNF:
//!
//! ```bnf
//! heading_atx ::= 1*6'#' [ 1*space_or_tab text [ 1*space_or_tab 1*'#' ] ] *space_or_tab
//!
//! text ::= code - eol
//! space_or_tab ::= ' ' | '\t'
//! ```
//!
//! Headings in markdown relate to the `<h1>` through `<h6>` elements in HTML.
//! See [*§ 4.3.6 The `h1`, `h2`, `h3`, `h4`, `h5`, and `h6` elements* in the
//! HTML spec][html] for more info.
//!
//! `CommonMark` introduced the requirement on whitespace existing after the
//! opening sequence and before text.
//! In older markdown versions, this was not required, and headings would form
//! without it.
//!
//! In markdown, it is also possible to create headings with a
//! [heading (setext)][heading_setext] construct.
//! The benefit of setext headings is that their text can include line endings,
//! and by extensions also hard breaks (e.g., with
//! [hard break (escape)][hard_break_escape]).
//! However, their limit is that they cannot form `<h3>` through `<h6>`
//! headings.
//! Due to this limitation, it is recommended to use atx headings.
//!
//! > 🏛 **Background**: the word *setext* originates from a small markup
//! > language by Ian Feldman from 1991.
//! > See [*§ Setext* on Wikipedia][wiki-setext] for more info.
//! > The word *atx* originates from a tiny markup language by Aaron Swartz
//! > from 2002.
//! > See [*§ atx, the true structured text format* on `aaronsw.com`][atx] for
//! > more info.
//!
//! ## Tokens
//!
//! *   [`HeadingAtx`][Token::HeadingAtx]
//! *   [`HeadingAtxSequence`][Token::HeadingAtxSequence]
//! *   [`HeadingAtxText`][Token::HeadingAtxText]
//! *   [`SpaceOrTab`][Token::SpaceOrTab]
//!
//! ## References
//!
//! *   [`heading-atx.js` in `micromark`](https://github.com/micromark/micromark/blob/main/packages/micromark-core-commonmark/dev/lib/heading-atx.js)
//! *   [*§ 4.2 ATX headings* in `CommonMark`](https://spec.commonmark.org/0.30/#atx-headings)
//!
//! [flow]: crate::content::flow
//! [heading_setext]: crate::construct::heading_setext
//! [hard_break_escape]: crate::construct::hard_break_escape
//! [html]: https://html.spec.whatwg.org/multipage/sections.html#the-h1,-h2,-h3,-h4,-h5,-and-h6-elements
//! [wiki-setext]: https://en.wikipedia.org/wiki/Setext
//! [atx]: http://www.aaronsw.com/2002/atx/

use super::partial_space_or_tab::{space_or_tab, space_or_tab_min_max};
use crate::constant::{HEADING_ATX_OPENING_FENCE_SIZE_MAX, TAB_SIZE};
use crate::token::Token;
use crate::tokenizer::{ContentType, Event, EventType, State, Tokenizer};

/// Start of a heading (atx).
///
/// ```markdown
/// > | ## aa
///     ^
/// ```
pub fn start(tokenizer: &mut Tokenizer) -> State {
    let max = if tokenizer.parse_state.constructs.code_indented {
        TAB_SIZE - 1
    } else {
        usize::MAX
    };

    if tokenizer.parse_state.constructs.heading_atx {
        tokenizer.enter(Token::HeadingAtx);
        tokenizer.go(space_or_tab_min_max(0, max), before)(tokenizer)
    } else {
        State::Nok
    }
}

/// Start of a heading (atx), after whitespace.
///
/// ```markdown
/// > | ## aa
///     ^
/// ```
fn before(tokenizer: &mut Tokenizer) -> State {
    if Some('#') == tokenizer.current {
        tokenizer.enter(Token::HeadingAtxSequence);
        sequence_open(tokenizer, 0)
    } else {
        State::Nok
    }
}

/// In the opening sequence.
///
/// ```markdown
/// > | ## aa
///     ^
/// ```
fn sequence_open(tokenizer: &mut Tokenizer, rank: usize) -> State {
    match tokenizer.current {
        None | Some('\n') if rank > 0 => {
            tokenizer.exit(Token::HeadingAtxSequence);
            at_break(tokenizer)
        }
        Some('#') if rank < HEADING_ATX_OPENING_FENCE_SIZE_MAX => {
            tokenizer.consume();
            State::Fn(Box::new(move |tokenizer| {
                sequence_open(tokenizer, rank + 1)
            }))
        }
        _ if rank > 0 => {
            tokenizer.exit(Token::HeadingAtxSequence);
            tokenizer.go(space_or_tab(), at_break)(tokenizer)
        }
        _ => State::Nok,
    }
}

/// After something but before something else.
///
/// ```markdown
/// > | ## aa
///       ^
/// ```
fn at_break(tokenizer: &mut Tokenizer) -> State {
    match tokenizer.current {
        None | Some('\n') => {
            tokenizer.exit(Token::HeadingAtx);
            tokenizer.register_resolver("heading_atx".to_string(), Box::new(resolve));
            // Feel free to interrupt.
            tokenizer.interrupt = false;
            State::Ok
        }
        Some('\t' | ' ') => tokenizer.go(space_or_tab(), at_break)(tokenizer),
        Some('#') => {
            tokenizer.enter(Token::HeadingAtxSequence);
            further_sequence(tokenizer)
        }
        Some(_) => {
            tokenizer.enter_with_content(Token::Data, Some(ContentType::Text));
            data(tokenizer)
        }
    }
}

/// In a further sequence (after whitespace).
///
/// Could be normal “visible” hashes in the heading or a final sequence.
///
/// ```markdown
/// > | ## aa ##
///           ^
/// ```
fn further_sequence(tokenizer: &mut Tokenizer) -> State {
    if let Some('#') = tokenizer.current {
        tokenizer.consume();
        State::Fn(Box::new(further_sequence))
    } else {
        tokenizer.exit(Token::HeadingAtxSequence);
        at_break(tokenizer)
    }
}

/// In text.
///
/// ```markdown
/// > | ## aa
///        ^
/// ```
fn data(tokenizer: &mut Tokenizer) -> State {
    match tokenizer.current {
        // Note: `#` for closing sequence must be preceded by whitespace, otherwise it’s just text.
        None | Some('\t' | '\n' | ' ') => {
            tokenizer.exit(Token::Data);
            at_break(tokenizer)
        }
        _ => {
            tokenizer.consume();
            State::Fn(Box::new(data))
        }
    }
}

/// Resolve heading (atx).
pub fn resolve(tokenizer: &mut Tokenizer) {
    let mut index = 0;
    let mut heading_start: Option<usize> = None;
    let mut data_start: Option<usize> = None;
    let mut data_end: Option<usize> = None;

    while index < tokenizer.events.len() {
        let event = &tokenizer.events[index];

        if event.token_type == Token::HeadingAtx {
            if event.event_type == EventType::Enter {
                heading_start = Some(index);
            } else if let Some(start) = data_start {
                // If `start` is some, `end` is too.
                let end = data_end.unwrap();

                tokenizer.map.add(
                    start,
                    0,
                    vec![Event {
                        event_type: EventType::Enter,
                        token_type: Token::HeadingAtxText,
                        point: tokenizer.events[start].point.clone(),
                        link: None,
                    }],
                );

                // Remove everything between the start and the end.
                tokenizer.map.add(start + 1, end - start - 1, vec![]);

                tokenizer.map.add(
                    end + 1,
                    0,
                    vec![Event {
                        event_type: EventType::Exit,
                        token_type: Token::HeadingAtxText,
                        point: tokenizer.events[end].point.clone(),
                        link: None,
                    }],
                );

                heading_start = None;
                data_start = None;
                data_end = None;
            }
        } else if heading_start.is_some() && event.token_type == Token::Data {
            if event.event_type == EventType::Enter {
                if data_start.is_none() {
                    data_start = Some(index);
                }
            } else {
                data_end = Some(index);
            }
        }

        index += 1;
    }
}
