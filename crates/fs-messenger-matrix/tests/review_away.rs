//! A room that was left and is joined again is read down to the leave, not to its start: the cursor
//! remembers where it left (DESIGN.md, "Room states"). A raw server, not Matrix.

mod review_common;

use fs_messenger_core::Provider;
use review_common::support::server::{Reply, Server};
use review_common::{
    answer, cursor_json, error, from_the_start, ids, is_room, login, messages_for, ordinary,
    own_membership, page, plain, provider, room_id, timeline,
};
use serde_json::json;

fn server() -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    |path| match path {
        p if p.ends_with("/login") => login(),
        p if p.contains("since=s3") => answer("s9", &json!({})),
        p if p.contains("since=s2") => {
            let t = timeline(&[plain("$j", "back")], false, Some("pj"));
            answer("s3", &json!({"join": {room_id("a"): t}}))
        }
        p if p.contains("since=s1") => {
            let t = timeline(&[own_membership("$out", "leave")], false, Some("pl"));
            answer("s2", &json!({"leave": {room_id("a"): t}}))
        }
        p if p.contains("/sync") => {
            let t = timeline(&[plain("$a1", "hi")], false, None);
            answer("s1", &json!({"join": {room_id("a"): t}}))
        }
        p if p.contains("/messages")
            && is_room(p, "a")
            && p.contains("from=pj")
            && p.contains("to=s1") =>
        {
            page(&[plain("$away", "said while away")])
        }
        p if p.contains("/messages") => error(500, "M_UNKNOWN"),
        p => ordinary(p),
    }
}

#[tokio::test]
async fn away_then_join_reads_to_the_leave_not_the_start() {
    let server = Server::start(server()).await;
    let provider = provider(&server).await;
    let one = provider.fetch(None).await.unwrap();
    let two = provider.fetch(one.next.as_ref()).await.unwrap();
    assert_eq!(
        cursor_json(&two)["away"],
        json!([{"room": room_id("a"), "at": "s1"}])
    );
    let three = provider.fetch(two.next.as_ref()).await.unwrap();
    assert!(
        ids(&three).iter().any(|i| i.contains("$away")),
        "{:?}",
        ids(&three)
    );
    let asked = messages_for(&server, "a");
    assert!(from_the_start(&asked).is_empty(), "{asked:?}");
    let cursor = cursor_json(&three);
    assert_eq!(cursor.get("away"), None, "{cursor}");
    assert!(
        cursor["rooms"].to_string().contains("!a:x.test"),
        "{cursor}"
    );
}
