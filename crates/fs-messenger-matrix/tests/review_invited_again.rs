//! Seventh review, finding 3: a room the cursor knows, then removed and invited again (or knocked)
//! between two fetches, shows up in the answer under `invite` (or `knock`) only. What it said before
//! must still be read, down to the position, with or without a failed fetch in between. A raw
//! server, not Matrix; what tuwunel answers for an invited account is in `tests/server`.

mod review_common;

use fs_messenger_core::{FetchBatch, Provider, ProviderCursor};
use fs_messenger_matrix::{MatrixProvider, Password};
use review_common::support::server::{Reply, Server};
use review_common::{
    account, answer, error, is_room, login, ordinary, page, plain, room_id, support, timeline,
};
use serde_json::{Value, json};

/// `!a` is read at `s1`; at `s2` the account is invited to it again (`how`: `invite` or `knock`),
/// and, if `with_b`, `!b` joins, whose reading fails while it is `with_b`. `/messages` of `!a`
/// with `to=s1` answers `a_answer`; anything else for `!a` is an error, so a read that is not bounded
/// by the old position shows.
async fn server_for(how: &'static str, with_b: bool, a_answer: fn() -> Reply) -> Server {
    Server::start(move |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s2") => answer("s3", &json!({})),
        p if p.contains("since=s3") => answer("s9", &json!({})),
        p if p.contains("since=s9") => answer("s9", &json!({})),
        p if p.contains("since=s1") => {
            let state = if how == "invite" {
                "invite_state"
            } else {
                "knock_state"
            };
            let mut rooms = json!({how: {room_id("a"): {state: {"events": []}}}});
            if with_b {
                let b = timeline(&[plain("$b1", "hi")], false, Some("pb"));
                rooms["join"] = json!({room_id("b"): b});
            }
            answer("s2", &rooms)
        }
        p if p.contains("/sync") => {
            let t = timeline(&[plain("$a1", "hi")], false, None);
            answer("s1", &json!({"join": {room_id("a"): t}}))
        }
        p if p.contains("/messages") && is_room(p, "a") && p.contains("to=s1") => a_answer(),
        p if p.contains("/messages") && is_room(p, "a") => error(500, "M_UNKNOWN"),
        p if p.contains("/messages") => error(500, "M_UNKNOWN"),
        p => ordinary(p),
    })
    .await
}

fn said_while_away() -> Reply {
    page(&[plain("$a2", "said before the kick")])
}

fn forbidden() -> Reply {
    error(403, "M_FORBIDDEN")
}

async fn provider(server: &Server) -> MatrixProvider {
    let store = support::fresh_store().await;
    MatrixProvider::login(account(server), &Password::new("pw"), store)
        .await
        .unwrap()
}

fn ids(batch: &FetchBatch) -> Vec<String> {
    batch
        .messages
        .iter()
        .map(|m| m.aliases[0].id.clone())
        .collect()
}

fn cursor_json(cursor: Option<&ProviderCursor>) -> Value {
    serde_json::from_str(&cursor.unwrap().0).unwrap()
}

#[tokio::test]
async fn what_a_room_said_before_it_became_an_invitation_arrives() {
    for how in ["invite", "knock"] {
        let server = server_for(how, false, said_while_away).await;
        let provider = provider(&server).await;
        let one = provider.fetch(None).await.unwrap();
        let two = provider.fetch(one.next.as_ref()).await.unwrap();
        assert!(
            ids(&two).iter().any(|i| i.contains("$a2")),
            "{how}: lost {:?}",
            ids(&two)
        );
    }
}

#[tokio::test]
async fn the_same_after_a_failed_fetch() {
    for how in ["invite", "knock"] {
        let server = server_for(how, true, said_while_away).await;
        let provider = provider(&server).await;
        let one = provider.fetch(None).await.unwrap();
        assert!(
            provider.fetch(one.next.as_ref()).await.is_err(),
            "{how}: fetch 2 fails"
        );
        // `!b` stays broken: the replay must bring `$a2` even though the fetch fails again.
        // It is read in a fetch that goes through, so the server is asked once `!b` works.
        let _ = provider.fetch(one.next.as_ref()).await;
        let heard = server.heard();
        let asked_a = heard
            .iter()
            .any(|(p, _)| p.contains("/messages") && is_room(p, "a") && p.contains("to=s1"));
        assert!(asked_a, "{how}: the replay never asked for the room's past");
    }
}

#[tokio::test]
async fn the_room_leaves_the_list_so_that_a_join_reads_it_as_new() {
    let server = server_for("invite", false, said_while_away).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    assert!(
        cursor_json(one.next.as_ref())["rooms"]
            .to_string()
            .contains("!a:x.test")
    );
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    let cursor = cursor_json(two.next.as_ref());
    assert!(
        !cursor["rooms"].to_string().contains("!a:x.test"),
        "{cursor}"
    );
}

#[tokio::test]
async fn a_room_never_known_is_not_read_for_an_invitation() {
    let server = Server::start(|path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=") => answer(
            "s2",
            &json!({"invite": {room_id("c"): {"invite_state": {"events": []}}}}),
        ),
        p if p.contains("/sync") => answer("s1", &json!({})),
        p => ordinary(p),
    })
    .await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    provider.fetch(one.next.as_ref()).await.unwrap();
    let asked = server
        .heard()
        .into_iter()
        .filter(|(p, _)| p.contains("/messages"))
        .count();
    assert_eq!(asked, 0);
}

#[tokio::test]
async fn a_refusal_gives_that_room_up_by_name_and_does_not_block() {
    let server = server_for("invite", false, forbidden).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider
        .fetch(one.next.as_ref())
        .await
        .expect("a 403 must not fail the fetch");
    let cursor = cursor_json(two.next.as_ref());
    assert!(
        cursor["given_up"].to_string().contains("!a:x.test"),
        "{cursor}"
    );
    assert_eq!(cursor.get("gaps"), None, "{cursor}");
}
