//! Whether the account joined a room itself.
//!
//! Pattern: Specification: `is_own_join` is a pure predicate over an event's JSON; `any_own_join`
//! combines it by "any" over several events. Combining different predicates is not used.

use serde_json::Value;

/// Whether this event shows that the account joined the room itself: `m.room.member` whose `sender`
/// and `state_key` are the account, and either `membership` is `join` (a profile change counts), or it
/// is a `leave` whose `unsigned.prev_content` says `join` (our own leave proves the join before it; a
/// homeserver shows an invited-again account that leave, not the join). An invitation, a declined
/// invitation (`leave` after `invite`), a knock, a kick (sent by someone else) and a join someone
/// else sent do not count. The `unsigned` part is the account's own homeserver's: the same trust as
/// the `join` section of its `/sync`.
pub(crate) fn is_own_join(own: &str, json: &str) -> bool {
    let Ok(event) = serde_json::from_str::<Value>(json) else {
        return false;
    };
    let text = |path: &[&str]| {
        let mut at = &event;
        for step in path {
            at = at.get(step)?;
        }
        at.as_str()
    };
    if text(&["type"]) != Some("m.room.member")
        || text(&["sender"]) != Some(own)
        || text(&["state_key"]) != Some(own)
    {
        return false;
    }
    match text(&["content", "membership"]) {
        Some("join") => true,
        Some("leave") => text(&["unsigned", "prev_content", "membership"]) == Some("join"),
        _ => false,
    }
}

/// Whether any of these events is the account's own join.
pub(crate) fn any_own_join<'a>(own: &str, jsons: impl IntoIterator<Item = &'a str>) -> bool {
    jsons.into_iter().any(|json| is_own_join(own, json))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "@me:x";

    fn member(sender: &str, state_key: &str, membership: &str, previous: Option<&str>) -> String {
        let unsigned = previous.map_or_else(
            || serde_json::json!({}),
            |p| serde_json::json!({"prev_content": {"membership": p}}),
        );
        serde_json::json!({"type": "m.room.member", "sender": sender, "state_key": state_key,
            "content": {"membership": membership}, "unsigned": unsigned})
        .to_string()
    }

    #[test]
    fn only_the_own_join_counts() {
        assert!(is_own_join(ME, &member(ME, ME, "join", None)));
        assert!(
            is_own_join(ME, &member(ME, ME, "join", Some("join"))),
            "a profile change"
        );
        assert!(
            !is_own_join(ME, &member("@carol:x", ME, "join", None)),
            "sent by someone else"
        );
        assert!(
            !is_own_join(ME, &member(ME, "@carol:x", "join", None)),
            "somebody else's join"
        );
        assert!(!is_own_join(ME, &member("@carol:x", ME, "invite", None)));
        assert!(
            !is_own_join(ME, &member(ME, ME, "leave", Some("invite"))),
            "a declined invitation"
        );
        assert!(!is_own_join(ME, &member(ME, ME, "knock", None)));
    }

    #[test]
    fn an_own_leave_after_a_join_proves_the_join() {
        assert!(is_own_join(ME, &member(ME, ME, "leave", Some("join"))));
        assert!(
            !is_own_join(ME, &member("@carol:x", ME, "leave", Some("join"))),
            "a kick is not ours"
        );
        assert!(!is_own_join(ME, &member(ME, ME, "leave", None)));
    }

    #[test]
    fn what_is_no_member_event_or_no_json_does_not_count() {
        let message = r#"{"type":"m.room.message","sender":"@me:x","state_key":"@me:x","content":{"membership":"join"}}"#;
        assert!(!is_own_join(ME, message));
        assert!(!is_own_join(ME, "not json"));
        assert!(!is_own_join(ME, r#"{"type":"m.room.member"}"#));
        assert!(!is_own_join(
            ME,
            r#"{"type":"m.room.member","sender":1,"state_key":2,"content":[]}"#
        ));
    }

    #[test]
    fn any_looks_through_all() {
        let a = member("@carol:x", ME, "invite", None);
        let b = member(ME, ME, "join", None);
        assert!(any_own_join(ME, [a.as_str(), b.as_str()]));
        assert!(!any_own_join(ME, [a.as_str()]));
        assert!(!any_own_join(ME, []));
    }
}
