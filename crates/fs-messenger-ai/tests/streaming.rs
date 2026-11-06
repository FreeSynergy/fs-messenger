//! The contract's `TextChunk`: `created` first, offsets in UTF-8 bytes per part, whole code points, the
//! state event last. Reasoning apart from the answer. Usage where a server says, and never a cost it does
//! not state. These events come on the provider's own feed, since the core has no way for them yet.

mod common;

use common::wire::{Step, anthropic, llama, serve, sse_head, stream};
use common::{AI, World};
use fs_messenger_core::{DeliveryState, Event, Messenger, SenderKind, TextChunk, TextPart, Usage};

fn chunks(events: &[Event], part: TextPart) -> Vec<TextChunk> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Text(c) if c.part == part => Some(c.clone()),
            _ => None,
        })
        .collect()
}

/// Every chunk starts where the last one ended, in bytes, and together they are the text.
fn joined(chunks: &[TextChunk]) -> String {
    let mut all = String::new();
    for chunk in chunks {
        assert_eq!(chunk.offset, all.len() as u64, "{chunks:?}");
        assert!(!chunk.text.is_empty());
        all.push_str(&chunk.text);
    }
    all
}

async fn usage_of(world: &World, done: &common::Finished) -> Usage {
    world.repo.usage(done.live.conversation).await.unwrap()
}

#[tokio::test]
async fn created_first_offsets_per_part_state_last() {
    let world = World::invited().await;
    let body = anthropic::start()
        + &anthropic::block(0, "thinking")
        + &anthropic::thinking(0, "Let me ")
        + &anthropic::thinking(0, "th\u{ef}nk")
        + &anthropic::stop_block(0)
        + &anthropic::block(1, "text")
        + &anthropic::text(1, "Gr\u{fc}\u{df}e ")
        + &anthropic::text(1, "\u{1f600}!")
        + &anthropic::stop_block(1)
        + &anthropic::end("end_turn", 15);
    let server = serve(vec![stream(body)]).await;
    let provider = world.anthropic(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    assert!(done.sent(), "{:?}", done.live.failure);

    let Event::Created(created) = &done.events[0] else {
        panic!("created comes first: {:?}", done.events[0]);
    };
    assert_eq!(created.state, DeliveryState::Sending);
    assert_eq!(created.envelope.id, done.live.message);
    assert_eq!(created.envelope.attributed_to.as_deref(), Some(AI));
    assert_eq!(
        created.sender_kind,
        SenderKind::Unknown,
        "the provider claims nothing; the guard's lookup comes with the stored message"
    );
    assert_eq!(created.job_ref, done.answer().summary.job_ref, "the link");
    let Event::State(end) = done.events.last().unwrap() else {
        panic!("the state event ends it");
    };
    assert_eq!(end.state, DeliveryState::Sent);
    assert_eq!(
        done.events
            .iter()
            .filter(|e| matches!(e, Event::State(_)))
            .count(),
        1
    );

    let body = joined(&chunks(&done.events, TextPart::Body));
    let reasoning = joined(&chunks(&done.events, TextPart::Reasoning));
    assert_eq!(body, "Gr\u{fc}\u{df}e \u{1f600}!");
    assert_eq!(reasoning, "Let me th\u{ef}nk");
    let answer = done.answer();
    assert_eq!(answer.body_text, body, "as stored");
    assert_eq!(
        answer.reasoning, reasoning,
        "as stored, apart from the answer"
    );
    assert!(!answer.body_text.contains("think"));
    assert_eq!(done.live.body, body);
    for chunk in chunks(&done.events, TextPart::Body) {
        assert_eq!(chunk.message, done.live.message);
        assert_eq!(chunk.conversation, done.live.conversation);
    }
}

#[tokio::test]
async fn reasoning_of_llama_cpp_is_kept_apart_too() {
    let world = World::invited().await;
    let body = llama::reasoning("hm ") + &llama::reasoning("yes") + &llama::answer(&["Yes."]);
    let server = serve(vec![stream(body)]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    assert!(done.sent(), "{:?}", done.live.failure);
    assert_eq!(done.answer().reasoning, "hm yes");
    assert_eq!(done.answer().body_text, "Yes.");
    assert_eq!(joined(&chunks(&done.events, TextPart::Reasoning)), "hm yes");
}

/// Characters split over TCP segments, one byte per write. What arrives is whole, and offsets count bytes.
#[tokio::test]
async fn multibyte_characters_split_across_chunks_arrive_whole() {
    for dialect_is_anthropic in [true, false] {
        let world = World::invited().await;
        let text = "\u{e4}\u{1f600}\u{4e2d}\u{6587}\u{1f468}\u{200d}\u{1f469}";
        let body = if dialect_is_anthropic {
            anthropic::answer(&[text, text])
        } else {
            llama::answer(&[text, text])
        };
        let server = serve(vec![vec![
            Step::Send(sse_head()),
            Step::Dribble(body.into_bytes()),
        ]])
        .await;
        let config = world.config(&server.origin);
        let provider = if dialect_is_anthropic {
            world.anthropic(config).await
        } else {
            world.llama(config).await
        };
        let done = world.turn(&provider, "hello").await;
        assert!(done.sent(), "{:?}", done.live.failure);
        assert_eq!(done.answer().body_text, text.repeat(2));
        let pieces = chunks(&done.events, TextPart::Body);
        assert_eq!(joined(&pieces), text.repeat(2));
        assert_eq!(pieces.len(), 2, "one per delta, however the bytes fell");
    }
}

#[tokio::test]
async fn a_late_listener_reads_the_text_so_far() {
    let world = World::invited().await;
    let first = sse_head()
        .into_iter()
        .chain(llama::text("so far").into_bytes())
        .collect();
    let server = serve(vec![vec![Step::Send(first), Step::Hang]]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    world.past_the_gate(&provider);
    let mut events = provider.live_events();
    let _ = world.person_sends(AI, "hello").await;
    let Event::Created(created) = events.recv().await.unwrap() else {
        panic!("created first");
    };
    let Event::Text(chunk) = events.recv().await.unwrap() else {
        panic!("then text");
    };
    assert_eq!(chunk.text, "so far");
    let live = provider.live_turn(created.envelope.id).unwrap();
    assert_eq!(
        live.state,
        DeliveryState::Sending,
        "sending while text arrives"
    );
    assert_eq!(live.body, "so far");
    assert!(live.stored.is_none());
    world.repo.stop(live.conversation).await.unwrap();
}

#[tokio::test]
async fn usage_of_anthropic_is_cumulative_and_adds_up_over_turns() {
    let world = World::invited().await;
    let server = serve(vec![
        stream(anthropic::answer(&["one"])),
        stream(anthropic::answer(&["two"])),
    ])
    .await;
    let provider = world.anthropic(world.config(&server.origin)).await;
    let first = world.turn(&provider, "q").await;
    let usage = usage_of(&world, &first).await;
    // input 25 from message_start; output_tokens from message_delta replaces the 1 said at the start.
    assert_eq!((usage.input_tokens, usage.output_tokens), (25, 15));
    assert_eq!(usage.context_used, 40);

    let mut events = provider.live_events();
    let _ = world.person_replies(first.answer().id(), "r").await;
    let second = world.finished(&provider, &mut events).await;
    let usage = usage_of(&world, &second).await;
    assert_eq!((usage.input_tokens, usage.output_tokens), (50, 30));
}

#[tokio::test]
async fn usage_of_llama_cpp_with_context_as_its_readme_says() {
    let world = World::invited().await;
    let server = serve(vec![stream(llama::answer(&["one"]))]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    let done = world.turn(&provider, "q").await;
    let usage = usage_of(&world, &done).await;
    assert_eq!((usage.input_tokens, usage.output_tokens), (44, 48));
    assert_eq!(
        usage.context_used,
        236 + 1 + 35,
        "prompt_n + cache_n + predicted_n"
    );
}

/// Neither API states a cost. A server that writes one is not believed, and none is computed.
#[tokio::test]
async fn cost_stays_not_known() {
    let world = World::invited().await;
    let body = anthropic::start()
        + "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\
\"usage\":{\"output_tokens\":5,\"cost_micros\":999,\"cost\":1.5,\"currency\":\"USD\"}}\n\n"
        + &anthropic::event(&serde_json::json!({"type": "message_stop"}));
    let server = serve(vec![stream(body)]).await;
    let provider = world.anthropic(world.config(&server.origin)).await;
    let done = world.turn(&provider, "q").await;
    let usage = usage_of(&world, &done).await;
    assert_eq!(usage.output_tokens, 5, "the usage itself did arrive");
    assert_eq!(usage.cost_micros, 0);
    assert_eq!(usage.currency, "");
}
