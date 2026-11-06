//! ADR 0054: a fresh program is stopped, and no AI participant exists until a person invites it.

mod common;

use common::{AI, FRIEND, OTHER_AI, OWNER, World, args};
use fs_messenger_core::{Ingested, Messenger, SenderKind, ToolCallRisk};
use fs_messenger_guard::{AiGrant, Gate, Guard, GuardError, GuardMode, Method, Refusal, Verdict};

#[tokio::test]
async fn a_fresh_program_is_stopped_and_stays_so_after_the_first_key() {
    let w = World::fresh().await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert!(w.record().await.is_none(), "reading writes nothing");
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::Stopped)
    );
    w.enrolment.enrol_owner(&w.owner.public()).await.unwrap();
    assert_eq!(w.engine.state().await.mode, GuardMode::Stopped);
    assert_eq!(World::doc_mode(&w.record().await.unwrap()), "stopped");
}

#[tokio::test]
async fn reopen_opens_it_and_a_restart_keeps_it_open() {
    let w = World::open().await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
    let w = w.restart().await;
    assert_eq!(w.engine.state().await.mode, GuardMode::Open);
}

#[tokio::test]
async fn nothing_exists_without_an_invitation_and_it_starts_with_no_release() {
    let w = World::open().await;
    assert_eq!(
        w.check("message.open", args("/")).await,
        Verdict::Refuse(Refusal::NotInvited)
    );
    w.invite(AI).await;
    let state = w.engine.state().await;
    assert_eq!(state.invited, [AI]);
    assert!(state.grants.is_empty(), "releases start empty");
    // Invited, and it still may not act: it can only ask.
    assert!(matches!(
        w.check("message.open", args("/")).await,
        Verdict::Ask(_)
    ));
    // Someone else is still nobody.
    let other = w
        .engine
        .check(
            &fs_messenger_guard::Caller::new(OTHER_AI),
            "message.open",
            &fs_messenger_guard::Arguments::none(),
        )
        .await;
    assert_eq!(other, Verdict::Refuse(Refusal::NotInvited));
}

#[tokio::test]
async fn an_invitation_needs_a_signature_of_the_person_for_exactly_this_profile() {
    let w = World::open().await;
    let for_other = w
        .owner
        .proof(&w.identity, Method::Invite, &[OTHER_AI.as_bytes()]);
    let refused = w.engine.invite(AI, &for_other).await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::BadProof))
    ));
    assert!(w.engine.state().await.invited.is_empty());
}

#[tokio::test]
async fn a_person_with_a_device_cannot_be_invited_as_an_ai() {
    let w = World::open().await;
    let proof = w
        .owner
        .proof(&w.identity, Method::Invite, &[OWNER.as_bytes()]);
    let refused = w.engine.invite(OWNER, &proof).await;
    assert!(matches!(
        refused,
        Err(GuardError::Refused(Refusal::IdentityConflict))
    ));
}

#[tokio::test]
async fn invite_and_release_are_refused_while_stopped() {
    let w = World::with_ai().await;
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let proof = w
        .owner
        .proof(&w.identity, Method::Invite, &[OTHER_AI.as_bytes()]);
    assert!(matches!(
        w.engine.invite(OTHER_AI, &proof).await,
        Err(GuardError::Refused(Refusal::Stopped))
    ));
    let grant = AiGrant {
        profile: AI.into(),
        risk: ToolCallRisk::ReadsOnly,
    };
    let proof = w.release_proof(AI, "readsOnly");
    assert!(matches!(
        w.engine.release(&grant, &proof).await,
        Err(GuardError::Refused(Refusal::Stopped))
    ));
}

#[tokio::test]
async fn the_rule_for_messages_of_an_agent_account() {
    let w = World::open().await;
    let agent = w.account("agents@local", "agent").await;
    // Uninvited: refused, nothing stored.
    let refused = w.deliver(agent.id, "m0", AI).await;
    assert!(matches!(
        refused,
        Err(fs_messenger_core::Error::GuardRefused(_))
    ));
    assert_eq!(
        w.repo
            .conversations(None, false, None)
            .await
            .unwrap()
            .items
            .len(),
        0
    );

    w.invite(AI).await;
    let Ingested::Stored(id) = w.deliver(agent.id, "m1", AI).await.unwrap() else {
        panic!()
    };
    let conv = w.repo.conversations(None, false, None).await.unwrap().items[0].id;
    assert_eq!(
        w.repo.message(conv, id).await.unwrap().summary.sender_kind,
        SenderKind::Ai,
        "chatting needs no release, and the invited read as AI"
    );

    w.engine.stop(&w.stop_proof()).await.unwrap();
    assert!(
        matches!(
            w.deliver(agent.id, "m2", AI).await,
            Err(fs_messenger_core::Error::GuardRefused(_))
        ),
        "stopped: every AI participant is refused"
    );
}

#[tokio::test]
async fn over_another_transport_the_uninvited_is_stored_and_marked_as_not_known() {
    let w = World::open().await;
    let mail = w.account("me@example.org", "mail").await;
    let Ingested::Stored(stranger) = w.deliver(mail.id, "m1", "bot@example.org").await.unwrap()
    else {
        panic!()
    };
    let conv = w.repo.conversations(None, false, None).await.unwrap().items[0].id;
    assert_eq!(
        w.repo
            .message(conv, stranger)
            .await
            .unwrap()
            .summary
            .sender_kind,
        SenderKind::Unknown,
        "stored, and never marked as a person"
    );
    w.invite(AI).await;
    let Ingested::Stored(invited) = w.deliver(mail.id, "m2", AI).await.unwrap() else {
        panic!()
    };
    assert_eq!(
        w.repo
            .message(conv, invited)
            .await
            .unwrap()
            .summary
            .sender_kind,
        SenderKind::Ai
    );
    // A stored message is no action, and mail is not lost: stopped, it still arrives, marked.
    w.engine.stop(&w.stop_proof()).await.unwrap();
    let Ingested::Stored(late) = w.deliver(mail.id, "m4", AI).await.unwrap() else {
        panic!()
    };
    assert_eq!(
        w.repo
            .message(conv, late)
            .await
            .unwrap()
            .summary
            .sender_kind,
        SenderKind::Ai
    );
    let Ingested::Stored(friend) = w.deliver(mail.id, "m3", FRIEND).await.unwrap() else {
        panic!()
    };
    assert_eq!(
        w.repo
            .message(conv, friend)
            .await
            .unwrap()
            .summary
            .sender_kind,
        SenderKind::Unknown
    );
}

#[tokio::test]
async fn gate_admits_a_message_only_from_the_invited_and_only_while_open() {
    let w = World::open().await;
    assert_eq!(w.engine.admit_message(AI).await, Err(Refusal::NotInvited));
    w.invite(AI).await;
    assert_eq!(w.engine.admit_message(AI).await, Ok(()));
    w.engine.stop(&w.stop_proof()).await.unwrap();
    assert_eq!(w.engine.admit_message(AI).await, Err(Refusal::Stopped));
}
