//! Who may release and revoke: who invited the participant, or the owner; and whom a question asks.

mod common;

use common::{AI, OTHER_AI, World, args};
use fs_messenger_core::ToolCallRisk as R;
use fs_messenger_guard::{AiGrant, Arguments, Caller, Gate, Guard, GuardError, Refusal, Verdict};

#[tokio::test]
async fn only_who_invited_it_or_the_owner_can_release_or_revoke() {
    use common::{Device, FRIEND};
    use fs_messenger_guard::Method;
    let w = World::with_ai().await;
    let friend = Device::new(FRIEND, 2);
    let key: String = friend.public().iter().map(|b| format!("{b:02x}")).collect();
    let add = w.owner.proof(
        &w.identity,
        Method::AddDevice,
        &[FRIEND.as_bytes(), key.as_bytes()],
    );
    w.enrolment
        .add_device(FRIEND, &friend.public(), &add)
        .await
        .unwrap();

    // The owner's AI: not the friend's to release or to revoke.
    let grant = AiGrant {
        profile: AI.into(),
        risk: R::ReadsOnly,
        program: "message".into(),
    };
    let proof = friend.proof(
        &w.identity,
        Method::Release,
        &[AI.as_bytes(), b"readsOnly", b"message"],
    );
    assert!(matches!(
        w.engine.release(&grant, &proof).await,
        Err(GuardError::Refused(Refusal::NotPermitted))
    ));
    w.release(AI, R::ReadsOnly, "readsOnly").await;
    let proof = friend.proof(&w.identity, Method::Revoke, &[AI.as_bytes(), b""]);
    assert!(matches!(
        w.engine.revoke(AI, &proof).await,
        Err(GuardError::Refused(Refusal::NotPermitted))
    ));
    assert_eq!(w.engine.state().await.grants.len(), 1);

    // The friend's own AI: the friend may, the owner may, and the friend is who gets asked.
    let proof = friend.proof(&w.identity, Method::Invite, &[OTHER_AI.as_bytes()]);
    w.engine.invite(OTHER_AI, &proof).await.unwrap();
    let grant = AiGrant {
        profile: OTHER_AI.into(),
        risk: R::Changes,
        program: "message".into(),
    };
    let proof = friend.proof(
        &w.identity,
        Method::Release,
        &[OTHER_AI.as_bytes(), b"changes", b"message"],
    );
    w.engine.release(&grant, &proof).await.unwrap();
    let input = Arguments::from_value(&args("/")).unwrap();
    let Verdict::Ask(q) = w
        .engine
        .check(&Caller::new(OTHER_AI), "message.delete", &input)
        .await
    else {
        panic!()
    };
    assert_eq!(q.call.asked_of, FRIEND);
    let proof = w
        .owner
        .proof(&w.identity, Method::Revoke, &[OTHER_AI.as_bytes(), b""]);
    w.engine.revoke(OTHER_AI, &proof).await.unwrap();
}
