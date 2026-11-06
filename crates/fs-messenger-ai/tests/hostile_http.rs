//! What a server can try before any text: errors, waits, redirects. The retry limit holds, a huge wait is
//! not sat out, and a redirect goes nowhere.

mod common;

use std::time::{Duration, Instant};

use common::wire::{Step, answer, llama, serve, stream};
use common::{AI, KEY, World};
use fs_messenger_ai::{Limits, TurnFailure};
use fs_messenger_core::{Messenger, MessengerConfig};

fn quick() -> Limits {
    Limits {
        backoff: Duration::from_millis(10),
        ..Limits::default()
    }
}

fn limited(wait: &str) -> Vec<Step> {
    let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#;
    answer("429 Too Many Requests", &[("retry-after", wait)], body)
}

#[tokio::test]
async fn a_huge_wait_ends_the_turn_at_once_and_names_it() {
    let world = World::invited().await;
    let server = serve(vec![limited("86400"), stream(llama::answer(&["never"]))]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    let started = Instant::now();
    let done = world.turn(&provider, "hello").await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        done.failure(),
        TurnFailure::RateLimited {
            wait_seconds: 86400
        }
    );
    assert_eq!(server.requests().len(), 1, "no second request");
}

#[tokio::test]
async fn a_small_wait_is_obeyed_and_then_it_goes_on() {
    let world = World::invited().await;
    let server = serve(vec![limited("1"), stream(llama::answer(&["fine"]))]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    let started = Instant::now();
    let done = world.turn(&provider, "hello").await;
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert!(done.sent(), "{:?}", done.live.failure);
    assert_eq!(done.answer().body_text, "fine");
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn the_retry_limit_holds() {
    let world = World::invited().await;
    let replies = vec![answer("503 Service Unavailable", &[], ""); 6];
    let server = serve(replies).await;
    let mut config = world.config(&server.origin);
    config.limits = quick();
    let provider = world.llama(config).await;
    let done = world.turn(&provider, "hello").await;
    assert!(matches!(
        done.failure(),
        TurnFailure::Http { status: 503, .. }
    ));
    assert_eq!(server.requests().len(), 3, "the first and two retries");
}

/// A date, a negative number or nonsense is no wait: the backoff applies, and the limit holds.
#[tokio::test]
async fn a_retry_after_that_is_no_number_counts_as_not_said() {
    for bad in [
        "Wed, 21 Oct 2099 07:28:00 GMT",
        "-5",
        "soon",
        "99999999999999999999999",
    ] {
        let world = World::invited().await;
        let server = serve(vec![limited(bad), stream(llama::answer(&["fine"]))]).await;
        let mut config = world.config(&server.origin);
        config.limits = quick();
        let provider = world.llama(config).await;
        let done = world.turn(&provider, "hello").await;
        assert!(done.sent(), "{bad}: {:?}", done.live.failure);
    }
}

#[tokio::test]
async fn what_cannot_get_better_is_not_retried() {
    for (status, kind) in [
        ("400 Bad Request", "invalid_request_error"),
        ("401 Unauthorized", "authentication_error"),
        ("404 Not Found", "not_found_error"),
    ] {
        let world = World::invited().await;
        let body = format!(r#"{{"error":{{"type":"{kind}","message":"no"}}}}"#);
        let server = serve(vec![
            answer(status, &[], &body),
            stream(llama::answer(&["x"])),
        ])
        .await;
        let provider = world.llama(world.config(&server.origin)).await;
        let done = world.turn(&provider, "hello").await;
        assert_eq!(server.requests().len(), 1, "{status}");
        let TurnFailure::Http { kind: said, .. } = done.failure() else {
            panic!("{status}");
        };
        assert_eq!(said, kind);
    }
}

#[tokio::test]
async fn a_broken_stream_is_not_sent_again() {
    let world = World::invited().await;
    let server = serve(vec![
        stream(llama::text("half")),
        stream(llama::answer(&["never"])),
    ])
    .await;
    let provider = world.llama(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    assert_eq!(done.failure(), TurnFailure::BrokeOff);
    assert_eq!(
        server.requests().len(),
        1,
        "a model bills for what it wrote"
    );
}

#[tokio::test]
async fn a_redirect_is_refused_and_the_other_side_hears_nothing() {
    for status in [
        "301 Moved",
        "302 Found",
        "307 Temporary Redirect",
        "308 Permanent",
    ] {
        let world = World::invited().await;
        let elsewhere = serve(vec![stream(llama::answer(&["stolen"]))]).await;
        let location = format!("{}/v1/messages", elsewhere.origin);
        let server = serve(vec![answer(status, &[("location", location.as_str())], "")]).await;
        let provider = world.anthropic(world.config(&server.origin)).await;
        let done = world.turn(&provider, "secret text").await;
        assert!(
            matches!(done.failure(), TurnFailure::Redirected(_)),
            "{status}"
        );
        assert_eq!(server.requests().len(), 1);
        assert!(
            server.requests()[0].headers["authorization"].contains(KEY),
            "the control: the key did go to the configured origin"
        );
        assert_eq!(elsewhere.requests().len(), 0, "{status}");
        assert_eq!(
            elsewhere
                .counters
                .connections
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }
}

#[tokio::test]
async fn a_server_that_never_answers_ends_at_the_idle_limit() {
    let world = World::invited().await;
    let server = serve(vec![vec![Step::Hang]]).await;
    let mut config = world.config(&server.origin);
    config.limits = Limits {
        idle: Duration::from_millis(200),
        ..Limits::default()
    };
    let provider = world.llama(config).await;
    let done = world.turn(&provider, "hello").await;
    assert_eq!(done.failure(), TurnFailure::IdleTimeout);
    assert!(
        done.answer()
            .summary
            .envelope
            .name
            .starts_with("Answer cut off")
    );
}

#[tokio::test]
async fn a_request_over_the_limit_stays_here() {
    let world = World::invited().await;
    let server = serve(vec![stream(llama::answer(&["never"]))]).await;
    let mut config = world.config(&server.origin);
    config.limits = Limits {
        request_bytes: 100,
        ..Limits::default()
    };
    let provider = world.llama(config).await;
    let done = world.turn(&provider, &"x".repeat(500)).await;
    assert_eq!(done.failure(), TurnFailure::RequestTooLarge);
    assert_eq!(server.requests().len(), 0);
}

#[tokio::test]
async fn nobody_listening_on_the_port_is_a_named_failure() {
    let world = World::invited().await;
    let free = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", free.local_addr().unwrap());
    drop(free);
    let mut config = world.config(&origin);
    config.limits = quick();
    let provider = world.llama(config).await;
    let done = world.turn(&provider, "hello").await;
    assert_eq!(done.failure(), TurnFailure::Connection);
    let page = world.repo.conversations(None, false, None).await.unwrap();
    assert_eq!(page.items.len(), 1, "the person sees what became of it");
}

/// A program that set its limits above the core's gets a stub in the store and `failed` here, no success.
#[tokio::test]
async fn an_answer_the_core_only_keeps_a_stub_of_reads_failed() {
    let mut config = MessengerConfig::new(common::OWNER);
    config.max_message_bytes = 1024;
    let world = World::with_config(config).await;
    world.invite(AI).await;
    let server = serve(vec![stream(llama::answer(&[&"x".repeat(5000)]))]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    assert_eq!(done.failure(), TurnFailure::ResponseTooLarge);
    assert!(done.answer().summary.too_large > 0);
    assert_eq!(
        done.live.body.len(),
        5000,
        "the text stays in the live view"
    );
}
