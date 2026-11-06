//! A stream that breaks, repeats, lies or never ends: it ends as `failed`, and the text so far is kept. The
//! stored message says so in its name, because the core gives a provider no way to store `failed`; the
//! state the core stores is `delivered`, and these tests pin that down so it cannot go unnoticed.

mod common;

use std::time::Duration;

use common::wire::{Reply, Step, answer, anthropic, llama, serve, sse_head, stream};
use common::{Finished, World};
use fs_messenger_ai::{Limits, TurnFailure};
use fs_messenger_core::{DeliveryState, Event};

const KEPT: &str = "kept text";

async fn anthropic_turn(reply: Reply, limits: Limits) -> Finished {
    let world = World::invited().await;
    let server = serve(vec![reply]).await;
    let mut config = world.config(&server.origin);
    config.limits = limits;
    let provider = world.anthropic(config).await;
    world.turn(&provider, "hello").await
}

async fn llama_turn(reply: Reply, limits: Limits) -> Finished {
    let world = World::invited().await;
    let server = serve(vec![reply]).await;
    let mut config = world.config(&server.origin);
    config.limits = limits;
    let provider = world.llama(config).await;
    world.turn(&provider, "hello").await
}

fn begun() -> String {
    anthropic::start() + &anthropic::block(0, "text") + &anthropic::text(0, KEPT)
}

fn assert_failed_with_text(done: &Finished, failure: &TurnFailure) {
    assert_eq!(&done.failure(), failure);
    assert_eq!(done.live.state, DeliveryState::Failed);
    assert!(
        matches!(
            done.events.last(),
            Some(Event::State(s)) if s.state == DeliveryState::Failed
        ),
        "the state event ends it"
    );
    assert_eq!(done.live.body, KEPT);
    let answer = done.answer();
    assert_eq!(answer.body_text, KEPT, "the text is kept");
    assert!(
        answer.summary.envelope.name.starts_with("Answer cut off:"),
        "{}",
        answer.summary.envelope.name
    );
    assert_eq!(
        answer.summary.state,
        DeliveryState::Delivered,
        "what the core stores today; DESIGN.md, Wanted"
    );
}

#[tokio::test]
async fn a_stream_that_breaks_off() {
    let done = anthropic_turn(stream(begun()), Limits::default()).await;
    assert_failed_with_text(&done, &TurnFailure::BrokeOff);
}

#[tokio::test]
async fn a_chunked_body_torn_in_the_middle() {
    let event = begun();
    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                transfer-encoding: chunked\r\n\r\n";
    let bytes = format!("{head}{:x}\r\n{event}\r\n1000\r\ntorn", event.len());
    let done = anthropic_turn(vec![Step::Send(bytes.into_bytes())], Limits::default()).await;
    assert_failed_with_text(&done, &TurnFailure::BrokeOff);
}

#[tokio::test]
async fn a_stream_that_starts_over() {
    let body = begun() + &anthropic::start() + &anthropic::block(0, "text");
    let done = anthropic_turn(stream(body), Limits::default()).await;
    assert!(matches!(done.failure(), TurnFailure::Repeated(_)));
    assert_eq!(done.answer().body_text, KEPT);
}

#[tokio::test]
async fn a_block_that_comes_twice() {
    let body = begun() + &anthropic::stop_block(0) + &anthropic::block(0, "text");
    let done = anthropic_turn(stream(body), Limits::default()).await;
    assert!(matches!(done.failure(), TurnFailure::Repeated(_)));
    assert_eq!(done.answer().body_text, KEPT);
}

#[tokio::test]
async fn chunks_of_another_completion_are_a_repeat() {
    let foreign = llama::text(KEPT)
        + &format!(
            "data: {}\n\n",
            serde_json::json!({"id": "chatcmpl-OTHER", "choices": [
                {"index": 0, "delta": {"content": "from elsewhere"}, "finish_reason": null}]})
        );
    let done = llama_turn(stream(foreign), Limits::default()).await;
    assert!(matches!(done.failure(), TurnFailure::Repeated(_)));
    assert_eq!(done.answer().body_text, KEPT);
}

#[tokio::test]
async fn text_after_the_finish_is_a_repeat() {
    let body = llama::text(KEPT)
        + &llama::chunk(&serde_json::json!({}), Some("stop"))
        + &llama::text("more");
    let done = llama_turn(stream(body), Limits::default()).await;
    assert!(matches!(done.failure(), TurnFailure::Repeated(_)));
    assert_eq!(done.answer().body_text, KEPT);
}

#[tokio::test]
async fn a_stream_that_stops_moving_ends_at_the_idle_limit() {
    let mut first = sse_head();
    first.extend(begun().into_bytes());
    let limits = Limits {
        idle: Duration::from_millis(200),
        ..Limits::default()
    };
    let done = anthropic_turn(vec![Step::Send(first), Step::Hang], limits).await;
    assert_failed_with_text(&done, &TurnFailure::IdleTimeout);
}

/// Each byte is news, so the idle limit never fires. The turn limit does.
#[tokio::test]
async fn a_slow_trickle_ends_at_the_turn_limit() {
    let mut first = sse_head();
    first.extend(begun().into_bytes());
    let limits = Limits {
        idle: Duration::from_secs(5),
        turn: Duration::from_millis(600),
        ..Limits::default()
    };
    let trickle = Step::Flood {
        piece: b": ping\n\n".to_vec(),
        every: Duration::from_millis(20),
    };
    let done = anthropic_turn(vec![Step::Send(first), trickle], limits).await;
    assert_eq!(done.failure(), TurnFailure::TurnTimeout);
    assert_eq!(done.answer().body_text, KEPT);
}

#[tokio::test]
async fn an_oversized_stream_stops_at_the_limit() {
    let limits = Limits {
        response_bytes: 64 * 1024,
        ..Limits::default()
    };
    let mut first = sse_head();
    first.extend(begun().into_bytes());
    let flood = Step::Flood {
        piece: anthropic::text(0, &"x".repeat(1000)).into_bytes(),
        every: Duration::from_millis(1),
    };
    let done = anthropic_turn(vec![Step::Send(first), flood], limits).await;
    assert_eq!(done.failure(), TurnFailure::ResponseTooLarge);
    assert!(done.answer().body_text.starts_with(KEPT));
    assert!(done.answer().body_text.len() < 64 * 1024);
}

#[tokio::test]
async fn one_large_chunk_over_the_limit_keeps_the_text_before_it() {
    let limits = Limits {
        response_bytes: 2048,
        ..Limits::default()
    };
    let body = begun() + &anthropic::text(0, &"x".repeat(50_000));
    let done = anthropic_turn(stream(body), limits).await;
    assert_failed_with_text(&done, &TurnFailure::ResponseTooLarge);
}

#[tokio::test]
async fn a_line_without_end_stops_at_the_event_limit() {
    let limits = Limits {
        event_bytes: 4096,
        ..Limits::default()
    };
    let body = begun() + "data: " + &"y".repeat(10_000);
    let done = anthropic_turn(stream(body), limits).await;
    assert!(matches!(done.failure(), TurnFailure::Malformed(_)));
    assert_eq!(done.answer().body_text, KEPT);
}

#[tokio::test]
async fn malformed_events_end_it() {
    for bad in [
        "data: {not json\n\n",
        "data: [1,2]\n\n",
        "data: {\"no\":\"type\"}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":7,\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":5}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"x\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":9}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"\\ud83d\"}}\n\n",
    ] {
        let done = anthropic_turn(stream(begun() + bad), Limits::default()).await;
        assert!(matches!(done.failure(), TurnFailure::Malformed(_)), "{bad}");
        assert_eq!(done.answer().body_text, KEPT, "{bad}");
    }
}

#[tokio::test]
async fn invalid_utf8_ends_it_without_a_replacement_character() {
    let mut bytes = begun().into_bytes();
    bytes.extend_from_slice(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"\xff\xfe\"}}\n\n");
    let done = anthropic_turn(stream(bytes), Limits::default()).await;
    assert_failed_with_text(&done, &TurnFailure::InvalidUtf8);
    assert!(!done.answer().body_text.contains('\u{fffd}'));
}
