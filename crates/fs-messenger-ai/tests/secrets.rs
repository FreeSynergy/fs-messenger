//! The API key: in no `Debug`, no error, no event, no activity, no file. A server that echoes it does not
//! get it stored or shown. Every needle check has a control that shows it can find the needle.

mod common;

use std::path::Path;

use common::wire::{answer, anthropic, serve, stream};
use common::{AI, KEY, World};
use fs_messenger_ai::{ApiKey, Endpoint, ModelConfig, ModelProvider};
use fs_store::{ActivityFilter, Store};

fn holds(bytes: &[u8], needle: &str) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// The whole log, as far as the store's API shows it: `Debug` and JSON of every activity.
async fn whole_log(world: &World) -> Vec<u8> {
    let store: &dyn Store = world.messages.as_ref();
    let log = store.activities(&ActivityFilter::default()).await.unwrap();
    let mut all = format!("{log:?}").into_bytes();
    all.extend(serde_json::to_vec(&log).unwrap());
    all
}

fn files_of(dir: &Path) -> Vec<u8> {
    let mut all = vec![];
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            all.extend(std::fs::read(path).unwrap());
        }
    }
    all
}

#[test]
fn no_configuration_prints_the_key() {
    let mut config =
        ModelConfig::new(AI, "m", Endpoint::parse("https://example.org").unwrap()).unwrap();
    config.api_key = Some(ApiKey::new(KEY));
    let shown = format!("{config:?} {config:#?} {:?}", config.api_key);
    assert!(!shown.contains(KEY), "{shown}");
    assert!(shown.contains("redacted"));
}

#[tokio::test]
async fn the_key_goes_to_the_configured_origin_as_its_one_header() {
    let world = World::invited().await;
    let server = serve(vec![stream(anthropic::answer(&["ok"]))]).await;
    let provider = world.anthropic(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    assert!(done.sent());
    let request = &server.requests()[0];
    let carrying: Vec<&String> = request
        .headers
        .iter()
        .filter(|(_, v)| v.contains(KEY))
        .map(|(k, _)| k)
        .collect();
    assert_eq!(carrying, ["authorization"]);
    assert!(!request.body.contains(KEY));
    assert!(!request.line.contains(KEY));
}

/// The server writes the key back in everything it can: split over deltas too, in reasoning, in a tool's
/// name, id and arguments. Nothing of it is shown, stored, logged or on disk. The control text is.
#[tokio::test]
async fn an_echo_of_the_key_is_kept_nowhere() {
    let world = World::invited().await;
    let (start, end) = KEY.split_at(7);
    let body = anthropic::start()
        + &anthropic::block(0, "thinking")
        + &anthropic::thinking(0, &format!("think {KEY}"))
        + &anthropic::stop_block(0)
        + &anthropic::block(1, "text")
        + &anthropic::text(1, &format!("CONTROL-MARK whole {KEY} and split "))
        + &anthropic::text(1, start)
        + &anthropic::text(1, end)
        + &anthropic::text(1, " end")
        + &anthropic::stop_block(1)
        + &anthropic::tool(2, KEY, &format!("name-{KEY}"))
        + &anthropic::arguments(2, &format!("{{\"k\":\"{KEY}\"}}"))
        + &anthropic::stop_block(2)
        + &anthropic::end("tool_use", 1);
    let server = serve(vec![stream(body)]).await;
    let provider = world.anthropic(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    assert!(done.sent(), "{:?}", done.live.failure);

    assert_eq!(
        done.answer().body_text,
        "CONTROL-MARK whole [redacted] and split [redacted] end"
    );
    let events = format!("{:?}", done.events);
    assert!(events.contains("CONTROL-MARK"), "the control");
    assert!(!events.contains(KEY), "{events}");
    assert!(!format!("{:?}", done.live).contains(KEY));
    let message = format!("{:?}", done.answer());
    assert!(message.contains("CONTROL-MARK"), "the control");
    assert!(!message.contains(KEY), "{message}");

    let log = whole_log(&world).await;
    assert!(
        holds(&log, "CONTROL-MARK"),
        "the control: the log shows text"
    );
    assert!(!holds(&log, KEY));
    // The files of an encrypted store do not show the control either, so this only says: not in the clear.
    assert!(!holds(&files_of(world.dir.path()), KEY));
}

/// The key in an error body, or an error type. The failure has only a status and a short safe name.
#[tokio::test]
async fn an_echo_of_the_key_in_an_error_goes_nowhere() {
    let world = World::invited().await;
    let body = format!(
        r#"{{"type":"error","error":{{"type":"{KEY}","message":"your key {KEY} is bad"}},"request_id":"{KEY}"}}"#
    );
    let server = serve(vec![answer(
        "401 Unauthorized",
        &[("x-echo", KEY), ("www-authenticate", KEY)],
        &body,
    )])
    .await;
    let provider = world.anthropic(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    let failure = done.failure();
    let shown = format!("{failure} {failure:?} {:?} {:?}", done.live, done.events);
    assert!(shown.contains("401"), "the control: {shown}");
    assert!(!shown.contains(KEY), "{shown}");
    let message = format!("{:?}", done.answer());
    assert!(!message.contains(KEY), "{message}");
    assert!(!holds(&whole_log(&world).await, KEY));
}

#[tokio::test]
async fn an_echo_in_a_stream_error_goes_nowhere() {
    let world = World::invited().await;
    let body = anthropic::start()
        + &format!(
            "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"{KEY}\",\"message\":\"{KEY}\"}}}}\n\n"
        );
    let server = serve(vec![stream(body)]).await;
    let provider = world.anthropic(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    let failure = done.failure();
    assert_eq!(failure.to_string(), "the server reported unknown");
    assert!(!format!("{:?}", done.answer()).contains(KEY));
}

/// A key in the very form of an error's short name still does not pass: this one found the leak.
#[tokio::test]
async fn a_key_that_looks_like_a_short_name_does_not_pass_either() {
    const LOOKS_LIKE_A_NAME: &str = "lowercase_key_only";
    let world = World::invited().await;
    for (body, what) in [
        (
            anthropic::start()
                + &format!(
                    "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"{LOOKS_LIKE_A_NAME}\"}}}}\n\n"
                ),
            "error",
        ),
        (
            anthropic::start() + &anthropic::end(LOOKS_LIKE_A_NAME, 1),
            "stop reason",
        ),
    ] {
        let server = serve(vec![stream(body)]).await;
        let mut config = world.config(&server.origin);
        config.api_key = Some(ApiKey::new(LOOKS_LIKE_A_NAME));
        let provider = ModelProvider::anthropic(config, world.link())
            .await
            .unwrap();
        let provider = std::sync::Arc::new(provider);
        let done = world.turn(&provider, "hello").await;
        let shown = format!("{} {:?} {:?}", done.failure(), done.live, done.answer());
        assert!(!shown.contains(LOOKS_LIKE_A_NAME), "{what}: {shown}");
    }
    assert!(!holds(&whole_log(&world).await, LOOKS_LIKE_A_NAME));
}

/// A provider refused at the start says nothing of the key either.
#[tokio::test]
async fn a_key_that_fits_no_header_is_refused_without_being_shown() {
    let world = World::invited().await;
    let mut config = world.config("http://127.0.0.1:9");
    config.api_key = Some(ApiKey::new(format!("{KEY}\r\nx-evil: 1")));
    let Err(error) = ModelProvider::anthropic(config, world.link()).await else {
        panic!("refused");
    };
    let shown = format!("{error} {error:?}");
    assert!(!shown.contains(KEY), "{shown}");
}

/// The provider's own error strings, through the core's `Error`.
#[tokio::test]
async fn refusals_of_a_send_do_not_carry_the_key() {
    let world = World::uninvited().await;
    let server = serve(vec![]).await;
    let provider = world.anthropic(world.config(&server.origin)).await;
    world.past_the_gate(&provider);
    let error = world.person_sends(AI, "hello").await.unwrap_err();
    assert!(!format!("{error} {error:?}").contains(KEY));
    assert!(!holds(&whole_log(&world).await, KEY));
}
