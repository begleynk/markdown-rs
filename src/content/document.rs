//! The document content type.
//!
//! **Document** represents the containers, such as block quotes and lists,
//! which structure the document and contain other sections.
//!
//! The constructs found in flow are:
//!
//! *   [Block quote][crate::construct::block_quote]
//! *   [List][crate::construct::list]

use crate::construct::{
    block_quote::{cont as block_quote_cont, start as block_quote},
    list::{cont as list_item_const, start as list_item},
};
use crate::content::flow::start as flow;
use crate::parser::ParseState;
use crate::subtokenize::subtokenize;
use crate::token::Token;
use crate::tokenizer::{
    Container, ContainerState, Event, EventType, Point, State, StateFn, Tokenizer,
};
use crate::util::{
    normalize_identifier::normalize_identifier,
    skip,
    slice::{Position, Slice},
};

/// Phases where we can exit containers.
#[derive(Debug, PartialEq)]
enum Phase {
    /// After parsing a line of lazy flow which resulted in something that
    /// exits containers before the line.
    ///
    /// ```markdown
    ///   | * a
    /// > | ```js
    ///          ^
    ///   | b
    ///   | ```
    /// ```
    After,
    /// When a new container replaces an existing container.
    ///
    /// ```markdown
    ///   | * a
    /// > | > b
    ///     ^
    /// ```
    Prefix,
    /// After everything.
    ///
    /// ```markdown
    /// > | * a
    ///        ^
    /// ```
    Eof,
}

/// State needed to parse document.
struct DocumentInfo {
    /// Number of containers that have continued.
    continued: usize,
    /// Index into `tokenizer.events` we need to track.
    index: usize,
    /// Events of containers added back later.
    inject: Vec<(Vec<Event>, Vec<Event>)>,
    /// The value of the previous line of flow’s `interrupt`.
    interrupt_before: bool,
    /// Whether the previous line of flow was a paragraph.
    paragraph_before: bool,
    /// Current containers.
    stack: Vec<ContainerState>,
    /// Current flow state function.
    next: Box<StateFn>,
}

/// Turn `codes` as the document content type into events.
pub fn document(parse_state: &mut ParseState, point: Point) -> Vec<Event> {
    let mut tokenizer = Tokenizer::new(point, parse_state);

    let state = tokenizer.push(0, parse_state.chars.len(), Box::new(before));
    tokenizer.flush(state, true);

    let mut index = 0;
    let mut definitions = vec![];

    while index < tokenizer.events.len() {
        let event = &tokenizer.events[index];

        if event.event_type == EventType::Exit && event.token_type == Token::DefinitionLabelString {
            // To do: when we operate on u8, we can use a `to_str` here as we
            // don‘t need virtual spaces.
            let id = normalize_identifier(
                &Slice::from_position(
                    &tokenizer.parse_state.chars,
                    &Position::from_exit_event(&tokenizer.events, index),
                )
                .serialize(),
            );

            if !definitions.contains(&id) {
                definitions.push(id);
            }
        }

        index += 1;
    }

    let mut events = tokenizer.events;

    parse_state.definitions = definitions;

    while !subtokenize(&mut events, parse_state) {}

    events
}

/// At the beginning.
///
/// Perhaps a BOM?
///
/// ```markdown
/// > | a
///     ^
/// ```
fn before(tokenizer: &mut Tokenizer) -> State {
    match tokenizer.current {
        Some('\u{FEFF}') => {
            tokenizer.enter(Token::ByteOrderMark);
            tokenizer.consume();
            tokenizer.exit(Token::ByteOrderMark);
            State::Fn(Box::new(start))
        }
        _ => start(tokenizer),
    }
}

/// Before document.
//
/// ```markdown
/// > | * a
///     ^
///   | > b
/// ```
fn start(tokenizer: &mut Tokenizer) -> State {
    let info = DocumentInfo {
        index: 0,
        continued: 0,
        inject: vec![],
        next: Box::new(flow),
        paragraph_before: false,
        interrupt_before: false,
        stack: vec![],
    };
    line_start(tokenizer, info)
}

/// Start of a line.
//
/// ```markdown
/// > | * a
///     ^
/// > | > b
///     ^
/// ```
fn line_start(tokenizer: &mut Tokenizer, mut info: DocumentInfo) -> State {
    info.index = tokenizer.events.len();
    info.inject.push((vec![], vec![]));
    info.continued = 0;
    // Containers would only be interrupting if we’ve continued.
    tokenizer.interrupt = false;
    container_existing_before(tokenizer, info)
}

/// Before existing containers.
//
/// ```markdown
///   | * a
/// > | > b
///     ^
/// ```
fn container_existing_before(tokenizer: &mut Tokenizer, mut info: DocumentInfo) -> State {
    // If there are more existing containers, check whether the next one continues.
    if info.continued < info.stack.len() {
        let container = info.stack.remove(info.continued);
        let cont = match container.kind {
            Container::BlockQuote => block_quote_cont,
            Container::ListItem => list_item_const,
        };

        tokenizer.container = Some(container);
        tokenizer.attempt(cont, move |ok| {
            if ok {
                Box::new(|t| container_existing_after(t, info))
            } else {
                Box::new(|t| container_existing_missing(t, info))
            }
        })(tokenizer)
    }
    // Otherwise, check new containers.
    else {
        container_new_before(tokenizer, info)
    }
}

/// At a missing, existing containers.
//
/// ```markdown
///   | * a
/// > | > b
///     ^
/// ```
fn container_existing_missing(tokenizer: &mut Tokenizer, mut info: DocumentInfo) -> State {
    let container = tokenizer.container.take().unwrap();
    info.stack.insert(info.continued, container);
    container_new_before(tokenizer, info)
}

/// After an existing container.
//
/// ```markdown
///   | * a
/// > |   b
///       ^
/// ```
fn container_existing_after(tokenizer: &mut Tokenizer, mut info: DocumentInfo) -> State {
    let container = tokenizer.container.take().unwrap();
    info.stack.insert(info.continued, container);
    info.continued += 1;
    container_existing_before(tokenizer, info)
}

/// Before a new container.
//
/// ```markdown
/// > | * a
///     ^
/// > | > b
///     ^
/// ```
fn container_new_before(tokenizer: &mut Tokenizer, info: DocumentInfo) -> State {
    // If we have completely continued, restore the flow’s past `interrupt`
    // status.
    if info.continued == info.stack.len() {
        tokenizer.interrupt = info.interrupt_before;

        // …and if we’re in a concrete construct, new containers can’t “pierce”
        // into them.
        if tokenizer.concrete {
            return containers_after(tokenizer, info);
        }
    }

    // Check for a new container.
    // Block quote?
    tokenizer.container = Some(ContainerState {
        kind: Container::BlockQuote,
        blank_initial: false,
        size: 0,
    });

    tokenizer.attempt(block_quote, move |ok| {
        if ok {
            Box::new(|t| container_new_after(t, info))
        } else {
            Box::new(|tokenizer| {
                // List item?
                tokenizer.container = Some(ContainerState {
                    kind: Container::ListItem,
                    blank_initial: false,
                    size: 0,
                });

                tokenizer.attempt(list_item, |ok| {
                    Box::new(move |t| {
                        if ok {
                            container_new_after(t, info)
                        } else {
                            containers_after(t, info)
                        }
                    })
                })(tokenizer)
            })
        }
    })(tokenizer)
}

/// After a new container.
//
/// ```markdown
/// > | * a
///       ^
/// > | > b
///       ^
/// ```
fn container_new_after(tokenizer: &mut Tokenizer, mut info: DocumentInfo) -> State {
    let container = tokenizer.container.take().unwrap();

    // Remove from the event stack.
    // We’ll properly add exits at different points manually.
    let token_type = match container.kind {
        Container::BlockQuote => Token::BlockQuote,
        Container::ListItem => Token::ListItem,
    };

    let mut stack_index = tokenizer.stack.len();
    let mut found = false;

    while stack_index > 0 {
        stack_index -= 1;

        if tokenizer.stack[stack_index] == token_type {
            tokenizer.stack.remove(stack_index);
            found = true;
            break;
        }
    }

    assert!(found, "expected to find container token to exit");

    // If we did not continue all existing containers, and there is a new one,
    // close the flow and those containers.
    if info.continued != info.stack.len() {
        info = exit_containers(tokenizer, info, &Phase::Prefix);
    }

    // Try another new container.
    info.stack.push(container);
    info.continued += 1;
    info.interrupt_before = false;
    tokenizer.interrupt = false;
    container_new_before(tokenizer, info)
}

/// After containers, before flow.
//
/// ```markdown
/// > | * a
///       ^
/// > | > b
///       ^
/// ```
fn containers_after(tokenizer: &mut Tokenizer, mut info: DocumentInfo) -> State {
    // Store the container events we parsed.
    info.inject
        .last_mut()
        .unwrap()
        .0
        .append(&mut tokenizer.events.split_off(info.index));

    tokenizer.lazy = info.continued != info.stack.len();
    tokenizer.interrupt = info.interrupt_before;
    tokenizer.define_skip_current();

    let state = info.next;
    info.next = Box::new(flow);

    // Parse flow, pausing after eols.
    tokenizer.go_until(
        state,
        |code| matches!(code, Some('\n')),
        move |state| Box::new(move |t| flow_end(t, info, state)),
    )(tokenizer)
}

/// After flow (after eol or at eof).
//
/// ```markdown
///   | * a
/// > | > b
///     ^  ^
/// ```
fn flow_end(tokenizer: &mut Tokenizer, mut info: DocumentInfo, result: State) -> State {
    let paragraph = !tokenizer.events.is_empty()
        && tokenizer.events[skip::opt_back(
            &tokenizer.events,
            tokenizer.events.len() - 1,
            &[Token::LineEnding],
        )]
        .token_type
            == Token::Paragraph;

    if tokenizer.lazy && info.paragraph_before && paragraph {
        info.continued = info.stack.len();
    }

    if info.continued != info.stack.len() {
        info = exit_containers(tokenizer, info, &Phase::After);
    }

    info.paragraph_before = paragraph;
    info.interrupt_before = tokenizer.interrupt;

    match result {
        State::Ok => {
            if !info.stack.is_empty() {
                info.continued = 0;
                info = exit_containers(tokenizer, info, &Phase::Eof);
            }

            resolve(tokenizer, &mut info);
            result
        }
        State::Nok => unreachable!("unexpected `nok` from flow"),
        State::Fn(func) => {
            info.next = func;
            line_start(tokenizer, info)
        }
    }
}

/// Close containers (and flow if needed).
fn exit_containers(
    tokenizer: &mut Tokenizer,
    mut info: DocumentInfo,
    phase: &Phase,
) -> DocumentInfo {
    let mut stack_close = info.stack.split_off(info.continued);

    // So, we’re at the end of a line, but we need to close the *previous* line.
    if *phase != Phase::Eof {
        tokenizer.define_skip_current();
        let mut current_events = tokenizer.events.split_off(info.index);
        let next = info.next;
        info.next = Box::new(flow); // This is weird but Rust needs a function there.
        tokenizer.flush(State::Fn(next), false);

        if *phase == Phase::Prefix {
            info.index = tokenizer.events.len();
        }

        tokenizer.events.append(&mut current_events);
    }

    let mut exits = Vec::with_capacity(stack_close.len());

    while !stack_close.is_empty() {
        let container = stack_close.pop().unwrap();
        let token_type = match container.kind {
            Container::BlockQuote => Token::BlockQuote,
            Container::ListItem => Token::ListItem,
        };

        exits.push(Event {
            event_type: EventType::Exit,
            token_type: token_type.clone(),
            // Note: positions are fixed later.
            point: tokenizer.point.clone(),
            link: None,
        });
    }

    let index = info.inject.len() - (if *phase == Phase::Eof { 1 } else { 2 });
    info.inject[index].1.append(&mut exits);
    info.interrupt_before = false;

    info
}

// Inject the container events.
fn resolve(tokenizer: &mut Tokenizer, info: &mut DocumentInfo) {
    let mut index = 0;
    let mut inject = info.inject.split_off(0);
    inject.reverse();
    let mut first_line_ending_in_run = None;

    while let Some((before, mut after)) = inject.pop() {
        if !before.is_empty() {
            first_line_ending_in_run = None;
            tokenizer.map.add(index, 0, before);
        }

        while index < tokenizer.events.len() {
            let event = &tokenizer.events[index];

            if event.token_type == Token::LineEnding || event.token_type == Token::BlankLineEnding {
                if event.event_type == EventType::Enter {
                    first_line_ending_in_run = first_line_ending_in_run.or(Some(index));
                } else {
                    index += 1;
                    break;
                }
            } else if event.token_type == Token::SpaceOrTab {
                // Empty to allow whitespace in blank lines.
            } else if first_line_ending_in_run.is_some() {
                first_line_ending_in_run = None;
            }

            index += 1;
        }

        let point_rel = if let Some(index) = first_line_ending_in_run {
            &tokenizer.events[index].point
        } else {
            &tokenizer.point
        };

        let close_index = first_line_ending_in_run.unwrap_or(index);

        let mut subevent_index = 0;
        while subevent_index < after.len() {
            after[subevent_index].point = point_rel.clone();
            subevent_index += 1;
        }

        tokenizer.map.add(close_index, 0, after);
    }

    tokenizer.map.consume(&mut tokenizer.events);
}
