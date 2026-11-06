//! How a stream may end, and what counts as a good end. Everything else is `failed`, with the text kept.

mod common;

use common::wire::{Reply, answer, anthropic, llama, serve, stream};
use common::{Finished, World};
use fs_messenger_ai::{Limits, TurnFailure};
use fs_messenger_core::DeliveryState;

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
    assert_eq!(done.answer().body_text, KEPT, "the text is kept");
}

#[tokio::test]
async fn text_before_the_start_is_malformed_and_unknown_events_are_not() {
    let done = anthropic_turn(stream(anthropic::text(0, "x")), Limits::default()).await;
    assert!(matches!(done.failure(), TurnFailure::Malformed(_)));
    assert_eq!(done.answer().body_text, "");
    // The versioning policy: new event types may be added, and new delta types.
    let body = anthropic::start()
        + "event: something_new\ndata: {\"type\":\"something_new\"}\n\n"
        + &anthropic::event(&serde_json::json!({"type": "ping"}))
        + &anthropic::block(0, "text")
        + &anthropic::event(
            &serde_json::json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "new_delta", "x": 1}}),
        )
        + &anthropic::text(0, KEPT)
        + &anthropic::stop_block(0)
        + &anthropic::end("end_turn", 1);
    let done = anthropic_turn(stream(body), Limits::default()).await;
    assert!(done.sent(), "{:?}", done.live.failure);
}

#[tokio::test]
async fn a_wrong_content_type_is_never_read() {
    let body = llama::answer(&["this must not show"]);
    let reply = answer("200 OK", &[("content-type", "text/html")], &body);
    let done = llama_turn(reply, Limits::default()).await;
    assert_eq!(done.failure(), TurnFailure::WrongContentType);
    assert_eq!(done.live.body, "");
    assert_eq!(done.answer().body_text, "");
    assert!(
        done.answer()
            .summary
            .envelope
            .name
            .contains("no event stream")
    );
}

#[tokio::test]
async fn an_error_in_the_stream_ends_it_by_its_short_name() {
    let body = begun()
        + "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
    let done = anthropic_turn(stream(body), Limits::default()).await;
    assert_failed_with_text(&done, &TurnFailure::ServerError("overloaded_error".into()));
    let body = llama::text(KEPT)
        + "data: {\"error\":{\"code\":500,\"message\":\"boom\",\"type\":\"server_error\"}}\n\n";
    let done = llama_turn(stream(body), Limits::default()).await;
    assert_failed_with_text(&done, &TurnFailure::ServerError("server_error".into()));
}

#[tokio::test]
async fn only_a_known_good_end_counts_as_sent() {
    for reason in [
        "max_tokens",
        "refusal",
        "pause_turn",
        "model_context_window_exceeded",
        "a_new_one",
    ] {
        let body = begun() + &anthropic::stop_block(0) + &anthropic::end(reason, 1);
        let done = anthropic_turn(stream(body), Limits::default()).await;
        assert_failed_with_text(&done, &TurnFailure::Cut(reason.into()));
    }
    let body = begun() + &anthropic::event(&serde_json::json!({"type": "message_stop"}));
    let done = anthropic_turn(stream(body), Limits::default()).await;
    assert!(matches!(done.failure(), TurnFailure::Cut(_)));

    let body = llama::text(KEPT) + &llama::end("length");
    let done = llama_turn(stream(body), Limits::default()).await;
    assert_failed_with_text(&done, &TurnFailure::Cut("length".into()));
    let done = llama_turn(
        stream(llama::text(KEPT) + "data: [DONE]\n\n"),
        Limits::default(),
    )
    .await;
    assert_failed_with_text(&done, &TurnFailure::BrokeOff);
    let done = llama_turn(stream(llama::text(KEPT)), Limits::default()).await;
    assert_failed_with_text(&done, &TurnFailure::BrokeOff);
}

#[tokio::test]
async fn a_finish_without_done_still_counts() {
    let body = llama::text(KEPT) + &llama::chunk(&serde_json::json!({}), Some("stop"));
    let done = llama_turn(stream(body), Limits::default()).await;
    assert!(done.sent(), "{:?}", done.live.failure);
}

#[tokio::test]
async fn reasoning_over_its_cap_is_cut_and_says_so_and_the_answer_goes_on() {
    let limits = Limits {
        reasoning_bytes: 10,
        ..Limits::default()
    };
    let body = anthropic::start()
        + &anthropic::block(0, "thinking")
        + &anthropic::thinking(0, &"r".repeat(100))
        + &anthropic::stop_block(0)
        + &anthropic::block(1, "text")
        + &anthropic::text(1, KEPT)
        + &anthropic::stop_block(1)
        + &anthropic::end("end_turn", 1);
    let done = anthropic_turn(stream(body), limits).await;
    assert!(done.sent());
    assert_eq!(
        done.answer().reasoning,
        format!("{}\n[reasoning cut off here]", "r".repeat(10))
    );
    assert_eq!(done.answer().body_text, KEPT);
}
