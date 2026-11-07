//! What is recorded and what must not be lost: the log, the risks, a stop in a conversation, a sync.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use common::rigs::Rig;
use common::{AI, World, args};
use fs_messenger_core::{
    AccountSetup, Encryption, ExternalId, FetchBatch, Ingest, Kind, MessageAction, Messenger,
    Provider, ProviderCursor, RawMessage, SenderKind, ToolCallOutcome, ToolCallRisk,
};
use fs_messenger_guard::{
    Allowance, Arguments, Caller, CapabilityRule, Gate, Guard, Verdict, capability_rule,
};
fn from_ai(id: &str) -> RawMessage {
    let mut raw = RawMessage::new(
        ExternalId::new("uid", id),
        ExternalId::new("thread", "t"),
        Kind::Chat,
        AI,
        "hello",
    );
    raw.encryption = Encryption::Mls;
    raw
}

#[tokio::test]
async fn the_log_shows_the_use_of_an_approval_and_a_release_and_the_end_of_a_call() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    assert!(matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(Allowance::Once { .. })
    ));
    w.engine
        .finished(&Caller::new(AI), &q.call.id, ToolCallOutcome::Done)
        .await
        .unwrap();
    let log = w.engine.log().await.unwrap();
    let used = log
        .iter()
        .find(|e| e.event == "approvalUsed")
        .expect("the use");
    assert_eq!(
        (used.call.as_str(), used.capability.as_str()),
        (q.call.id.as_str(), "message.delete")
    );
    let end = log.iter().find(|e| e.event == "finished").expect("the end");
    assert_eq!(end.outcome, "done");

    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let count = async || {
        w.engine
            .log()
            .await
            .unwrap()
            .iter()
            .filter(|e| e.event == "classUsed")
            .count()
    };
    assert_eq!(count().await, 0);
    for _ in 0..5 {
        assert!(matches!(
            w.check("message.open", args("/")).await,
            Verdict::Allow(_)
        ));
    }
    assert_eq!(count().await, 1, "bounded");
    w.clock.advance(chrono::Duration::minutes(2));
    assert!(matches!(
        w.check("message.open", args("/")).await,
        Verdict::Allow(_)
    ));
    assert_eq!(count().await, 2);
}

#[test]
fn the_risks_the_review_decided() {
    use CapabilityRule::{NeverForAi, Risk};
    assert_eq!(
        capability_rule("conversation.markRead"),
        Risk(ToolCallRisk::ActsForPerson)
    );
    assert_eq!(
        capability_rule("conversation.move"),
        Risk(ToolCallRisk::ActsForPerson)
    );
    assert_eq!(capability_rule("draft.save"), Risk(ToolCallRisk::Changes));
    assert_eq!(
        capability_rule("draft.change"),
        Risk(ToolCallRisk::Destroys)
    );
    for spelt in [
        "account.secret",
        "ACCOUNT.SECRET",
        " account.secret ",
        "Account.Secret",
    ] {
        assert_eq!(capability_rule(spelt), NeverForAi, "{spelt}");
    }
}

#[tokio::test]
async fn revoke_cancels_what_is_open_also_an_approval() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = w
        .owner
        .decide(&w.identity, q.message, &q.call, "allow-once");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    let proof = w.owner.proof(
        &w.identity,
        fs_messenger_guard::Method::Revoke,
        &[AI.as_bytes()],
    );
    w.engine.revoke(AI, &proof).await.unwrap();
    assert_eq!(w.shown(&q).await.outcome, ToolCallOutcome::Cancelled);
    assert!(!matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Allow(_)
    ));
}

#[tokio::test]
async fn a_stop_clears_what_the_retry_limit_tracks() {
    let w = World::with_ai().await;
    w.ask("message.delete", args("/m")).await;
    assert_eq!(w.engine.tracked_retries(), 1);
    w.engine.stop(&w.stop_proof()).await.unwrap();
    assert_eq!(w.engine.tracked_retries(), 0);
}

struct OneMessage;

#[async_trait]
impl Provider for OneMessage {
    async fn fetch(
        &self,
        _since: Option<&ProviderCursor>,
    ) -> fs_messenger_core::Result<FetchBatch> {
        Ok(FetchBatch {
            messages: vec![from_ai("m1")],
            next: Some(ProviderCursor("after-m1".into())),
            more: false,
        })
    }
}

#[tokio::test]
async fn a_guard_store_that_does_not_answer_reads_as_unknown_and_stalls_nothing() {
    let r = Rig::open().await;
    let account = r
        .repo
        .add_account(AccountSetup {
            address: "ai@local".into(),
            kind: "matrix".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    r.repo.register_provider(account.id, Arc::new(OneMessage));
    r.faulty.hang_loads.store(true, Ordering::SeqCst);
    let report = r.repo.sync(account.id).await.unwrap();
    assert_eq!(report.stored, 1, "the message is not lost");
    let conv = r.repo.conversations(None, false, None).await.unwrap().items[0].id;
    let shown = r.repo.messages(conv, None).await.unwrap().items;
    assert_eq!(
        shown[0].sender_kind,
        SenderKind::Unknown,
        "no answer in time reads as unknown, never as a person"
    );
}

#[tokio::test]
async fn conversation_stop_asks_the_provider_even_if_settling_fails() {
    use std::time::Duration;
    use tokio::sync::Notify;
    struct Counts(Arc<Notify>);
    #[async_trait]
    impl Provider for Counts {
        async fn stop_turns(
            &self,
            _: &fs_messenger_core::TurnScope,
        ) -> fs_messenger_core::Result<()> {
            self.0.notify_one();
            Ok(())
        }
    }
    // The state's write fails: its record cannot be settled, and the provider is still asked.
    let r = Rig::open().await;
    let account = r
        .repo
        .add_account(AccountSetup {
            address: "a@x".into(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    let asked = Arc::new(Notify::new());
    r.repo
        .register_provider(account.id, Arc::new(Counts(asked.clone())));
    let mut raw = from_ai("c1");
    raw.sender = "x@y".into();
    r.repo.ingest(account.id, raw).await.unwrap();
    let conv = r.repo.conversations(None, false, None).await.unwrap().items[0].id;
    let input = Arguments::none();
    assert!(matches!(
        r.engine
            .check(&Caller::in_conversation(AI, conv), "x.y", &input)
            .await,
        Verdict::Ask(_)
    ));
    r.faulty.fail_writes.store(true, Ordering::SeqCst);
    assert!(r.repo.stop(conv).await.is_err());
    tokio::time::timeout(Duration::from_secs(1), asked.notified())
        .await
        .expect("asked");
}
