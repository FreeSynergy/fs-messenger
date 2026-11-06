//! Fail-safe defaults, for old records too, and the provider rule (ADR 0045).

mod common;

use common::{account, conversation_of, raw, setup};
use fs_messenger_core::{
    AccountSetup, Encryption, Error, Ingest, Ingested, Kind, MessageAction, Messenger, Provider,
    SendReceipt, SenderKind, ToolCall, ToolCallChoice, ToolCallKind, ToolCallOption, ToolCallRisk,
};
use fs_store::ObjectId;
use serde_json::{from_value, json};

#[test]
fn unset_and_unknown_values_read_as_the_safe_ones() {
    let call: ToolCall = from_value(json!({})).unwrap();
    assert_eq!(call.risk, ToolCallRisk::Unknown);
    assert_eq!(call.kind, ToolCallKind::Unknown);
    assert!(call.options.is_empty() && call.arguments_digest.is_empty());
    let option: ToolCallOption = from_value(json!({"id": "x"})).unwrap();
    assert_eq!(option.choice, ToolCallChoice::Refuse);

    let future: ToolCall = from_value(json!({"risk": "newRisk", "kind": "newKind"})).unwrap();
    assert_eq!(
        (future.risk, future.kind),
        (ToolCallRisk::Unknown, ToolCallKind::Unknown)
    );
    let future: ToolCallOption = from_value(json!({"choice": "allowForever"})).unwrap();
    assert_eq!(future.choice, ToolCallChoice::Refuse);

    assert_eq!(Encryption::default(), Encryption::Unknown);
    assert_eq!(SenderKind::default(), SenderKind::Unknown);
    assert_eq!(
        from_value::<Encryption>(json!("quantum")).unwrap(),
        Encryption::Unknown
    );
    assert_eq!(SendReceipt::default().encryption, Encryption::Unknown);
    assert!(!Encryption::Unknown.is_end_to_end());
}

#[test]
fn encryption_uses_the_stored_names_and_reads_the_contract_name() {
    assert_eq!(
        serde_json::to_value(Encryption::TransportOnly).unwrap(),
        json!("transport-only")
    );
    let read: Encryption = from_value(json!("transportOnly")).unwrap();
    assert_eq!(read, Encryption::TransportOnly);
    for e in [
        Encryption::Mls,
        Encryption::Openpgp,
        Encryption::Megolm,
        Encryption::None,
        Encryption::Unknown,
    ] {
        assert_eq!(from_value::<Encryption>(json!(e.as_str())).unwrap(), e);
    }
}

#[tokio::test]
async fn an_old_record_without_those_fields_reads_safely_through_the_program() {
    let (repo, store) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let Ingested::Stored(id) = repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let mut old = store.get(id).await.unwrap().unwrap();
    old.content = json!({ "content": "old text", "toolCalls": [{ "id": "c1", "name": "n" }] });
    store.put(old, "old-version").await.unwrap();

    let m = repo.message(conv, id).await.unwrap();
    assert_eq!(m.summary.encryption, Encryption::Unknown);
    assert_eq!(m.summary.sender_kind, SenderKind::Unknown);
    assert_eq!(m.summary.kind, Kind::Mail);
    assert_eq!(m.body_media_type, "text/plain");
    assert_eq!(m.tool_calls[0].risk, ToolCallRisk::Unknown);
    assert_eq!(m.tool_calls[0].kind, ToolCallKind::Unknown);
}

#[tokio::test]
async fn a_provider_without_overrides_says_not_possible_everywhere() {
    struct Nothing;
    impl Provider for Nothing {}
    let p = Nothing;
    assert!(matches!(p.kind(), Err(Error::Unsupported(_))));
    assert!(matches!(p.fetch(None).await, Err(Error::Unsupported(_))));
    let Err(Error::Unsupported(u)) = p.encryption_plan(&[]).await else {
        panic!("expected not possible");
    };
    assert_eq!((u.capability(), u.status()), ("encryption.plan", 501));
    let caps = p.capabilities();
    for id in [
        "provider.kind",
        "message.fetch",
        "message.send",
        "encryption.plan",
    ] {
        assert!(caps.why_not(id).is_some(), "{id}");
    }
}

#[tokio::test]
async fn a_messenger_without_overrides_says_not_possible_and_the_repo_reports_the_truth() {
    struct Nothing;
    #[async_trait::async_trait]
    impl Messenger for Nothing {
        fn capabilities(&self) -> fs_interfaces::Capabilities {
            fs_interfaces::Capabilities::new("nothing")
        }
    }
    let id = ObjectId::new();
    assert!(matches!(
        Nothing.accounts().await,
        Err(Error::Unsupported(_))
    ));
    let acted = Nothing.act(id, id, MessageAction::Delete).await;
    assert!(matches!(acted, Err(Error::Unsupported(_))));

    let (repo, _) = setup().await;
    let caps = repo.capabilities();
    assert!(caps.is_supported("message.send") && caps.is_supported("toolcall.show"));
    for id in [
        "toolcall.decide",
        "turn.stop",
        "text.stream",
        "message.schedule",
        "rule.read",
    ] {
        assert!(caps.why_not(id).is_some(), "{id}");
    }
    assert!(matches!(repo.rules().await, Err(Error::Unsupported(_))));
}

#[tokio::test]
async fn accounts_refuse_secrets_and_duplicates_and_report_readiness() {
    let (repo, _) = setup().await;
    let setup_with = |secret: &str| AccountSetup {
        address: "a@example.org".into(),
        kind: "mail".into(),
        secret: secret.into(),
    };
    let Err(Error::Unsupported(u)) = repo.add_account(setup_with("hunter2")).await else {
        panic!("a secret has nowhere safe to go yet");
    };
    assert_eq!(u.capability(), "account.secret");
    assert!(!format!("{:?}", setup_with("hunter2")).contains("hunter2"));

    let a = repo.add_account(setup_with("")).await.unwrap();
    assert!(!a.encryption_ready);
    assert!(matches!(
        repo.add_account(setup_with("")).await,
        Err(Error::Conflict(_))
    ));
    repo.register_provider(a.id, common::TestProvider::new(Encryption::Mls, &[]));
    assert!(repo.accounts().await.unwrap()[0].encryption_ready);
    repo.remove_account(a.id).await.unwrap();
    assert!(repo.accounts().await.unwrap().is_empty());
}

#[tokio::test]
async fn usage_is_stored_as_given_and_zero_means_not_known() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    repo.ingest(me.id, raw("m1", "t", "x")).await.unwrap();
    let conv = conversation_of(&repo).await;
    assert_eq!(repo.usage(conv).await.unwrap(), Default::default());
    let usage = fs_messenger_core::Usage {
        input_tokens: 5,
        currency: "EUR".into(),
        ..Default::default()
    };
    repo.record_usage(conv, usage.clone()).await.unwrap();
    assert_eq!(repo.usage(conv).await.unwrap(), usage);
}
