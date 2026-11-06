//! ADR 0018, outgoing: only end-to-end leaves, and a refusal means nothing left.

mod common;

use common::{TestProvider, account, conversation_of, raw, setup, with_provider};
use fs_messenger_core::{
    DeliveryState, Encryption, Error, Ingest, Ingested, MessageAction, Messenger, Provider,
};
use fs_store::{Query, Store};
use std::sync::Arc;

async fn notes(store: &Arc<dyn Store>) -> usize {
    store.count(&Query::new().of_type("Note")).await.unwrap()
}

#[tokio::test]
async fn plans_without_end_to_end_are_refused_and_the_provider_is_never_called() {
    for plan in [
        Encryption::None,
        Encryption::TransportOnly,
        Encryption::Unknown,
    ] {
        let (repo, store) = setup().await;
        let (_, provider) = with_provider(&repo, plan, &[]).await;
        let mut d = repo.compose(None).await.unwrap();
        d.set_recipients(
            vec!["bob@example.org".into(), "eve@example.org".into()],
            vec![],
            vec![],
        )
        .await
        .unwrap();
        d.set_text("secret".into(), "text/plain".into())
            .await
            .unwrap();

        let Err(Error::EncryptionRefused {
            encryption,
            missing_keys,
        }) = d.send(None).await
        else {
            panic!("{plan:?} must be refused");
        };
        assert_eq!(encryption, plan);
        assert_eq!(missing_keys, ["bob@example.org", "eve@example.org"]);
        assert_eq!(provider.sent_count(), 0, "nothing left");
        assert_eq!(notes(&store).await, 0, "nothing became a message");
    }
}

#[tokio::test]
async fn the_error_names_who_lacks_a_key_also_on_a_good_plan_and_for_hidden_recipients() {
    let (repo, _) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut d = repo.compose(None).await.unwrap();
    let missing = d
        .set_recipients(
            vec!["bob@example.org".into()],
            vec!["carol@example.org".into()],
            vec!["hidden@example.org".into()],
        )
        .await
        .unwrap();
    assert_eq!(missing, ["carol@example.org", "hidden@example.org"]);
    d.set_text("x".into(), "text/plain".into()).await.unwrap();
    let err = d.send(None).await.unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("carol@example.org") && text.contains("hidden@example.org"),
        "{text}"
    );
    assert!(!text.contains("bob@"), "{text}");
    assert_eq!(provider.sent_count(), 0);
}

#[tokio::test]
async fn no_provider_or_one_without_a_plan_is_no_way() {
    struct Nothing;
    impl Provider for Nothing {}

    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    for attempt in 0..2 {
        if attempt == 1 {
            repo.register_provider(me.id, Arc::new(Nothing));
        }
        let mut d = repo.compose(None).await.unwrap();
        d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
            .await
            .unwrap();
        d.set_text("x".into(), "text/plain".into()).await.unwrap();
        let plan = d.encryption_plan().await.unwrap();
        assert_eq!(plan.encryption, Encryption::Unknown);
        assert_eq!(plan.missing_keys, ["bob@example.org"]);
        assert!(matches!(
            d.send(None).await,
            Err(Error::EncryptionRefused { .. })
        ));
    }
    assert_eq!(notes(&store).await, 0);
}

#[tokio::test]
async fn a_refused_draft_is_kept_and_goes_out_once_the_key_is_there() {
    let (repo, store) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Openpgp, &[]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("hello".into(), "text/markdown".into())
        .await
        .unwrap();
    assert!(d.send(None).await.is_err());
    assert_eq!(repo.drafts().await.unwrap().len(), 1);

    provider.keys.lock().unwrap().push("bob@example.org".into());
    let id = d.send(None).await.unwrap();

    let sent = provider.sent.lock().unwrap()[0].clone();
    assert_eq!(sent.encryption(), Encryption::Openpgp);
    assert_eq!(sent.to(), ["bob@example.org"]);
    assert_eq!(sent.text(), "hello");
    assert_eq!(sent.media_type(), "text/markdown");
    assert_eq!(sent.id(), id);

    assert!(repo.drafts().await.unwrap().is_empty());
    assert_eq!(notes(&store).await, 1);
    let conv = conversation_of(&repo).await;
    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.summary.state, DeliveryState::Sent);
    assert_eq!(m.summary.encryption, Encryption::Openpgp);
    assert!(!m.summary.unread && m.outgoing);
    assert!(d.send(None).await.is_err(), "a sent draft is over");
}

#[tokio::test]
async fn forwarding_runs_the_same_rule() {
    let (repo, _) = setup().await;
    let (me, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let Ingested::Stored(id) = repo
        .ingest(me.id, raw("m1", "t", "forward me"))
        .await
        .unwrap()
    else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let refused = repo
        .act(
            conv,
            id,
            MessageAction::Forward(vec!["eve@example.org".into()]),
        )
        .await;
    let Err(Error::EncryptionRefused { missing_keys, .. }) = refused else {
        panic!("forward must be refused without a key");
    };
    assert_eq!(missing_keys, ["eve@example.org"]);
    assert_eq!(provider.sent_count(), 0);

    repo.act(
        conv,
        id,
        MessageAction::Forward(vec!["bob@example.org".into()]),
    )
    .await
    .unwrap();
    assert_eq!(provider.sent.lock().unwrap()[0].text(), "forward me");
}

#[tokio::test]
async fn no_recipients_or_no_account_leave_nothing_either() {
    let (repo, _) = setup().await;
    let provider = TestProvider::new(Encryption::Mls, &[]);
    let a = account(&repo, "a@example.org").await;
    account(&repo, "b@example.org").await;
    repo.register_provider(a.id, provider.clone());
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    assert!(
        matches!(d.send(None).await, Err(Error::Invalid(_))),
        "two accounts: which one?"
    );
    d.set_account(a.id).await.unwrap();
    d.set_recipients(vec![], vec![], vec![]).await.unwrap();
    assert!(matches!(d.send(None).await, Err(Error::Invalid(_))));
    assert_eq!(provider.sent_count(), 0);
}
