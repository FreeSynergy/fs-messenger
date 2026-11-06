//! `stop_turns` really cancels the request: after it, no more bytes are consumed.

mod common;

use std::time::Duration;

use common::wire::{Reply, Scripted, Step, anthropic, llama, serve, sse_head, stream};
use common::{AI, Finished, World};
use fs_messenger_ai::{Limits, ModelProvider, TurnFailure};
use fs_messenger_core::{DeliveryState, Event, Messenger, Provider, TurnScope};
use std::sync::Arc;

/// Text, then the same piece for ever, until the connection is gone.
fn flood() -> Reply {
    let mut first = sse_head();
    first.extend(llama::text("before the stop ").into_bytes());
    vec![
        Step::Send(first),
        Step::Flood {
            piece: llama::text("more ").into_bytes(),
            every: Duration::from_millis(2),
        },
    ]
}

/// A turn that is in the middle of its stream.
async fn running(world: &World, server: &Scripted) -> Arc<ModelProvider> {
    let provider = world.llama(world.config(&server.origin)).await;
    world.past_the_gate(&provider);
    let _ = world.person_sends(AI, "hello").await;
    while server.written() < 2000 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    provider
}

async fn assert_bytes_stop(server: &Scripted) {
    // The server's next write after the close fails; give it the time for a few writes.
    for _ in 0..400 {
        if server.write_failed() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        server.write_failed(),
        "the connection must really be closed"
    );
    let then = server.written();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(server.written(), then, "nothing is written any more");
}

async fn conversation_of(world: &World) -> fs_store::ObjectId {
    world
        .repo
        .conversations(None, false, None)
        .await
        .unwrap()
        .items[0]
        .id
}

async fn ended(
    world: &World,
    provider: &Arc<ModelProvider>,
    events: &mut tokio::sync::broadcast::Receiver<Event>,
) -> Finished {
    world.finished(provider, events).await
}

#[tokio::test]
async fn stop_turns_closes_the_connection_and_keeps_the_text() {
    let world = World::invited().await;
    let server = serve(vec![flood()]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    world.past_the_gate(&provider);
    let mut events = provider.live_events();
    let _ = world.person_sends(AI, "hello").await;
    while server.written() < 2000 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    provider.stop_turns(&TurnScope::All).await.unwrap();
    assert_bytes_stop(&server).await;
    let done = ended(&world, &provider, &mut events).await;
    assert_eq!(done.failure(), TurnFailure::Stopped);
    assert_eq!(done.live.state, DeliveryState::Failed);
    assert!(done.answer().body_text.starts_with("before the stop "));
    assert_eq!(done.answer().body_text, done.live.body);
    assert!(done.answer().summary.envelope.name.contains("stopped"));
}

/// The way a person stops: `Messenger::stop`, through the guard, by the alias the core learned.
#[tokio::test]
async fn conversation_stop_reaches_the_request() {
    let world = World::invited().await;
    let server = serve(vec![flood()]).await;
    let _provider = running(&world, &server).await;
    let conversation = conversation_of(&world).await;
    world.repo.stop(conversation).await.unwrap();
    assert_bytes_stop(&server).await;
}

/// The emergency stop: the request ends. The guard is shut, so the answer is not stored; its text stays
/// in the live view only.
#[tokio::test]
async fn the_emergency_stop_reaches_the_request() {
    let world = World::invited().await;
    let server = serve(vec![flood()]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    world.past_the_gate(&provider);
    let mut events = provider.live_events();
    let _ = world.person_sends(AI, "hello").await;
    while server.written() < 2000 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    world.emergency_stop().await;
    assert_bytes_stop(&server).await;
    let done = ended(&world, &provider, &mut events).await;
    assert_eq!(done.failure(), TurnFailure::Stopped);
    assert!(
        done.stored.is_none(),
        "stopped means the guard stores nothing"
    );
    assert!(done.live.body.starts_with("before the stop "));
}

#[tokio::test]
async fn a_stop_ends_only_the_conversation_it_names() {
    let world = World::invited().await;
    let server = serve(vec![flood()]).await;
    let provider = running(&world, &server).await;
    let other = fs_messenger_core::ExternalId::new("model-conversation", "another");
    provider
        .stop_turns(&TurnScope::Conversation(vec![other]))
        .await
        .unwrap();
    let then = server.written();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(server.written() > then, "still flowing");
    assert!(!server.write_failed());
    provider.stop_turns(&TurnScope::All).await.unwrap();
    assert_bytes_stop(&server).await;
}

#[tokio::test]
async fn a_stop_with_nothing_running_is_fine() {
    let world = World::invited().await;
    let provider = world.llama(world.config("http://127.0.0.1:9")).await;
    provider.stop_turns(&TurnScope::All).await.unwrap();
}

/// A stop during a retry's wait does not sit the wait out, and sends nothing more.
#[tokio::test]
async fn a_stop_ends_the_wait_of_a_retry() {
    let world = World::invited().await;
    let limited = common::wire::answer("429 Too Many Requests", &[("retry-after", "20")], "");
    let server = serve(vec![limited, stream(llama::answer(&["never"]))]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    world.past_the_gate(&provider);
    let mut events = provider.live_events();
    let _ = world.person_sends(AI, "hello").await;
    server.until_requests(1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    provider.stop_turns(&TurnScope::All).await.unwrap();
    let done = tokio::time::timeout(
        Duration::from_secs(5),
        ended(&world, &provider, &mut events),
    )
    .await
    .expect("not after 20 seconds");
    assert_eq!(done.failure(), TurnFailure::Stopped);
    assert_eq!(server.requests().len(), 1);
}

/// A provider that cannot be stopped would be reported. This one answers only after the close.
#[tokio::test]
async fn the_answer_to_a_stop_means_the_connection_is_closed() {
    let world = World::invited().await;
    let reply = vec![Step::Send(sse_head()), Step::Hang];
    let server = serve(vec![reply]).await;
    let mut config = world.config(&server.origin);
    config.limits = Limits {
        idle: Duration::from_secs(60),
        ..Limits::default()
    };
    let provider = world.anthropic(config).await;
    world.past_the_gate(&provider);
    let mut events = provider.live_events();
    let _ = world.person_sends(AI, "hello").await;
    server.until_requests(1).await;
    provider.stop_turns(&TurnScope::All).await.unwrap();
    let done = ended(&world, &provider, &mut events).await;
    assert_eq!(done.failure(), TurnFailure::Stopped);
    let _ = anthropic::start();
}

#[tokio::test]
async fn after_a_stop_the_conversation_takes_a_next_turn() {
    let world = World::invited().await;
    let server = serve(vec![
        stream(llama::answer(&["first"])),
        flood(),
        stream(llama::answer(&["third"])),
    ])
    .await;
    let provider = world.llama(world.config(&server.origin)).await;
    let first = world.turn(&provider, "one").await;
    let mut events = provider.live_events();
    let _ = world.person_replies(first.answer().id(), "two").await;
    server.until_requests(2).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    world.repo.stop(first.live.conversation).await.unwrap();
    let second = ended(&world, &provider, &mut events).await;
    assert_eq!(second.failure(), TurnFailure::Stopped);
    let _ = world.person_replies(first.answer().id(), "three").await;
    let third = ended(&world, &provider, &mut events).await;
    assert!(third.sent(), "{:?}", third.live.failure);
    assert_eq!(third.answer().body_text, "third");
}
