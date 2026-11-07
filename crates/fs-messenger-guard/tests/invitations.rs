//! Invitations: withdrawn, yielding to a person, one whatever the spelling; what follows a refusal.

mod common;

use common::{AI, Device, FRIEND, OWNER, World, args};
use fs_messenger_core::{MessageAction, Messenger, SenderKind, ToolCallOutcome, ToolCallRisk};
use fs_messenger_guard::{AiGrant, Guard, GuardError, Method, Refusal, Verdict, id_key};
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[tokio::test]
async fn uninvite_takes_it_all_and_needs_who_invited_or_the_owner() {
    let w = World::with_ai().await;
    w.release(AI, ToolCallRisk::ReadsOnly, "readsOnly").await;
    let q = w.ask("message.delete", args("/m")).await;
    let friend = Device::new(FRIEND, 2);
    let key = hex(&friend.public());
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[FRIEND.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .add_device(FRIEND, &friend.public(), &add)
        .await
        .unwrap();
    let refused = w
        .engine
        .uninvite(
            AI,
            &friend.proof(&w.identity, Method::Uninvite, &[AI.as_bytes()]),
        )
        .await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::NotPermitted))
    ));

    let proof = w
        .owner
        .proof(&w.identity, Method::Uninvite, &[AI.as_bytes()]);
    w.engine.uninvite(AI, &proof).await.unwrap();
    let state = w.engine.state().await;
    assert!(state.invited.is_empty());
    assert!(state.grants.is_empty());
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::NotInvited)
    );
    assert_eq!(w.shown(&q).await.outcome, ToolCallOutcome::Cancelled);
    assert_eq!(
        w.engine.log().await.unwrap().last().unwrap().event,
        "uninvite"
    );
    // It can be invited again; nothing comes back with that.
    w.invite(AI).await;
    assert!(w.engine.state().await.grants.is_empty());
}

#[tokio::test]
async fn a_persons_identity_wins_over_someone_elses_invitation_of_it() {
    let w = World::open().await;
    let friend = Device::new(FRIEND, 2);
    let key = hex(&friend.public());
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[FRIEND.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .add_device(FRIEND, &friend.public(), &add)
        .await
        .unwrap();
    // The friend invites Carol's profile.
    const CAROL: &str = "https://local.invalid/users/carol";
    let invite = friend.proof(&w.identity, Method::Invite, &[CAROL.as_bytes()]);
    w.engine.invite(CAROL, &invite).await.unwrap();

    let carol = Device::new(CAROL, 4);
    let key = hex(&carol.public());
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[CAROL.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .add_device(CAROL, &carol.public(), &add)
        .await
        .unwrap();
    assert!(
        w.engine.state().await.invited.is_empty(),
        "the invitation yields"
    );
    // Carol signs as a person now, and cannot be invited again.
    let invite = friend.proof(&w.identity, Method::Invite, &[CAROL.as_bytes()]);
    assert!(matches!(
        w.engine.invite(CAROL, &invite).await,
        Err(GuardError::Refused(Refusal::IdentityConflict))
    ));
}

#[tokio::test]
async fn after_a_refusal_it_may_not_ask_for_a_while() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let d = w.owner.decide(&w.identity, q.message, &q.call, "refuse");
    w.repo
        .act(q.conversation, q.message, MessageAction::Decide(d))
        .await
        .unwrap();
    let before = w.record().await;
    assert_eq!(
        w.check("message.delete", args("/m")).await,
        Verdict::Refuse(Refusal::Paused)
    );
    assert_eq!(
        w.check("message.delete", args("/other")).await,
        Verdict::Refuse(Refusal::Paused)
    );
    assert_eq!(w.record().await, before);
    w.clock.advance(chrono::Duration::minutes(11));
    assert!(matches!(
        w.check("message.delete", args("/m")).await,
        Verdict::Ask(_)
    ));
}

#[tokio::test]
async fn identifiers_are_one_whatever_their_case() {
    let w = World::open().await;
    let shouted = "HTTPS://LOCAL.INVALID/AI/Helper ";
    let invite = w
        .owner
        .proof(&w.identity, Method::Invite, &[shouted.as_bytes()]);
    w.engine.invite(shouted, &invite).await.unwrap();
    assert_eq!(w.engine.state().await.invited, [id_key(shouted)]);
    assert!(
        matches!(w.check("message.open", args("/")).await, Verdict::Ask(_)),
        "the same AI"
    );
    // A key cannot be had under another spelling of an AI the owner invited.
    let ai = Device::new(shouted, 9);
    let key = hex(&ai.public());
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[AI.as_bytes(), key.as_bytes()],
    );
    assert!(matches!(
        w.enrolment.add_device(AI, &ai.public(), &add).await,
        Err(GuardError::Refused(Refusal::IdentityConflict))
    ));
    // Nor the owner under another spelling.
    let shouting_owner = OWNER.to_uppercase();
    let invite = w
        .owner
        .proof(&w.identity, Method::Invite, &[shouting_owner.as_bytes()]);
    assert!(matches!(
        w.engine.invite(&shouting_owner, &invite).await,
        Err(GuardError::Refused(Refusal::IdentityConflict))
    ));
    let grant = AiGrant {
        profile: AI.to_uppercase(),
        risk: ToolCallRisk::ReadsOnly,
        program: "message".into(),
        goes_to: "this device only".into(),
    };
    let name = AI.to_uppercase();
    let proof = w.owner.proof(
        &w.identity,
        Method::Release,
        &[
            name.as_bytes(),
            b"readsOnly",
            b"message",
            b"this device only",
        ],
    );
    w.engine.release(&grant, &proof).await.unwrap();
    assert!(matches!(
        w.check("message.open", args("/")).await,
        Verdict::Allow(_)
    ));
}

#[tokio::test]
async fn the_note_of_a_question_reads_as_an_ai() {
    let w = World::with_ai().await;
    let q = w.ask("message.delete", args("/m")).await;
    let m = w.repo.message(q.conversation, q.message).await.unwrap();
    assert_eq!(m.summary.sender_kind, SenderKind::Ai);
    assert_eq!(m.summary.envelope.attributed_to.as_deref(), Some(AI));
}
