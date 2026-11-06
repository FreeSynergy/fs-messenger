//! Encrypted only (ADR 0018) meets a model: what the plan says, and what the core does with it. Real core,
//! real guard. The first two tests are the state of the program today.

mod common;

use std::sync::Arc;

use common::wire::{anthropic, llama, serve, serve_tls, stream};
use common::{AI, KEY, MODEL, OTHER_AI, World};
use fs_messenger_ai::{ApiKey, DataPath, Endpoint, ModelConfig, ModelProvider, RemoteChoice};
use fs_messenger_core::{AccountSetup, Encryption, Error, Messenger, Provider};

fn real(world: &World, provider: &Arc<ModelProvider>) {
    let provider: Arc<dyn Provider> = provider.clone();
    world.repo.register_provider(world.account, provider);
}

#[tokio::test]
async fn the_core_refuses_a_loopback_model_and_nothing_reaches_the_wire() {
    let world = World::invited().await;
    let server = serve(vec![stream(llama::answer(&["never"]))]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    assert_eq!(provider.data_path(), DataPath::ThisDevice);
    real(&world, &provider);

    let refused = world.person_sends(AI, "hello").await;
    let Err(Error::EncryptionRefused {
        encryption,
        missing_keys,
    }) = refused
    else {
        panic!("{refused:?}");
    };
    assert_eq!(encryption, Encryption::None);
    assert_eq!(missing_keys, [AI]);
    assert_eq!(server.requests().len(), 0);
    assert_eq!(
        server
            .counters
            .connections
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let drafts = world.repo.drafts().await.unwrap();
    assert_eq!(drafts.len(), 1, "what the person wrote stays a draft");
    assert_eq!(drafts[0].text, "hello");
}

/// What the core reports as it stands, pinned so that a change shows. `DESIGN.md`, "Wanted".
#[tokio::test]
async fn the_core_says_ready_and_says_nothing_streams() {
    let world = World::invited().await;
    let provider = world.llama(world.config("http://127.0.0.1:9")).await;
    real(&world, &provider);
    let accounts = world.repo.accounts().await.unwrap();
    assert!(
        accounts[0].encryption_ready,
        "it reads ready because the provider can plan, though each plan is refused"
    );
    assert!(!world.repo.capabilities().is_supported("text.stream"));
}

#[tokio::test]
async fn the_core_refuses_a_remote_model_even_if_the_person_chose_it() {
    let world = World::invited().await;
    let server = serve_tls(vec![stream(anthropic::answer(&["never"]))]).await;
    let mut config = world.config(&server.origin);
    config.trust =
        fs_messenger_ai::TlsTrust::OnlyThisCertificateForTests(server.certificate.clone());
    let provider = world.anthropic(config).await;
    assert!(matches!(
        provider.data_path(),
        DataPath::LeavesDevice { .. }
    ));
    real(&world, &provider);
    provider.choose_remote(RemoteChoice::new(common::OWNER, "hello"));

    let refused = world.person_sends(AI, "hello").await;
    let Err(Error::EncryptionRefused {
        encryption,
        missing_keys,
    }) = refused
    else {
        panic!("{refused:?}");
    };
    assert_eq!(encryption, Encryption::TransportOnly);
    assert_eq!(missing_keys, [AI]);
    assert_eq!(server.requests().len(), 0);
}

#[tokio::test]
async fn the_plan_never_claims_end_to_end_and_names_every_recipient() {
    let world = World::invited().await;
    let local = world.llama(world.config("http://127.0.0.1:9")).await;
    let everyone = vec![AI.to_string(), OTHER_AI.to_string()];
    let plan = local.encryption_plan(&everyone).await.unwrap();
    assert_eq!(plan.encryption, Encryption::None);
    assert_eq!(plan.missing_keys, everyone);
    let mut remote = world.config("https://models.example.org");
    remote.api_key = Some(ApiKey::new(KEY));
    let remote = world.anthropic(remote).await;
    let plan = remote.encryption_plan(&everyone).await.unwrap();
    assert_eq!(plan.encryption, Encryption::TransportOnly);
    assert!(!plan.encryption.is_end_to_end());
}

/// Past the gate by a lie (labelled in `common`): the receipt still says the truth, and the core stores
/// that and says so. No stored state claims encryption.
#[tokio::test]
async fn a_plan_that_lies_cannot_make_the_provider_claim_encryption() {
    let world = World::invited().await;
    let server = serve(vec![stream(llama::answer(&["hi"]))]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    world.past_the_gate(&provider);
    let mut events = provider.live_events();
    let sent = world.person_sends(AI, "hello").await;
    let Err(Error::EncryptionDowngraded {
        message,
        encryption,
    }) = sent
    else {
        panic!("{sent:?}");
    };
    assert_eq!(encryption, Encryption::None);
    let finished = world.finished(&provider, &mut events).await;
    let person = world
        .repo
        .message(finished.live.conversation, message)
        .await
        .unwrap();
    assert_eq!(person.summary.encryption, Encryption::None);
    assert_eq!(finished.answer().summary.encryption, Encryption::None);
}

#[tokio::test]
async fn it_says_what_it_is_and_what_it_cannot() {
    let world = World::invited().await;
    let provider = world.llama(world.config("http://127.0.0.1:9")).await;
    assert_eq!(provider.kind().unwrap(), "agent");
    let caps = provider.capabilities();
    for id in ["message.send", "encryption.plan", "turn.stop"] {
        assert!(caps.is_supported(id), "{id}");
    }
    assert!(!caps.is_supported("message.fetch"));
    assert!(matches!(
        provider.fetch(None).await,
        Err(Error::Unsupported(_))
    ));
}

#[tokio::test]
async fn only_an_agent_account_takes_a_model() {
    let world = World::invited().await;
    let mail = world
        .repo
        .add_account(AccountSetup {
            address: "me@example.org".into(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await
        .unwrap();
    let mut link = world.link();
    link.account = mail.id;
    let config = world.config("http://127.0.0.1:9");
    assert!(
        ModelProvider::llama_cpp(config.clone(), link.clone())
            .await
            .is_err()
    );
    link.account = fs_store::ObjectId::new();
    assert!(ModelProvider::llama_cpp(config, link).await.is_err());
}

#[tokio::test]
async fn the_name_of_a_model_is_configuration_without_a_default() {
    let endpoint = Endpoint::parse("http://127.0.0.1:9").unwrap();
    assert!(ModelConfig::new(AI, "", endpoint.clone()).is_err());
    assert!(ModelConfig::new(AI, "  ", endpoint.clone()).is_err());
    assert!(ModelConfig::new("", MODEL, endpoint).is_err());
    let world = World::invited().await;
    let config = world.config("http://127.0.0.1:9");
    assert!(
        ModelProvider::anthropic(config, world.link())
            .await
            .is_err(),
        "this API needs a key"
    );
}
