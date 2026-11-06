//! How a model is attached, and what the core answers. **No network, no model and no key are used:** the
//! origin points at a port nobody is asked, because the core refuses before anything leaves.
//!
//! Run: `cargo run -p fs-messenger-ai --example wiring`. It generates a device key in this process only
//! because it is an example; a real private key stays on the person's device.

use std::sync::{Arc, Weak};

use ed25519_compact::{KeyPair, Seed};
use fs_messenger_ai::{Endpoint, Host, ModelConfig, ModelProvider, ProviderLink};
use fs_messenger_core::{AccountSetup, Messenger, MessengerConfig, MessengerRepo, Provider};
use fs_messenger_guard::{
    Guard, GuardConfig, GuardEngine, GuardProof, Method, TursoGuardRepository, signed_bytes,
};
use fs_store::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};

const OWNER: &str = "https://local.invalid/users/owner";
const AI: &str = "https://local.invalid/ai/helper";

async fn store(path: std::path::PathBuf) -> Arc<TursoStore> {
    let options = OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    };
    Arc::new(TursoStore::open_with(path, options).await.expect("a store"))
}

fn proof(pair: &KeyPair, engine: &[u8], method: Method, args: &[&[u8]], n: u8) -> GuardProof {
    let nonce = vec![n; 16];
    let bytes = signed_bytes(engine, method, OWNER, args, &nonce).expect("short fields");
    GuardProof {
        signer: OWNER.into(),
        nonce,
        proof: pair.sk.sign(&bytes, None).to_vec(),
    }
}

#[tokio::main]
async fn main() {
    let dir = tempfile::tempdir().expect("a folder");
    let messages = store(dir.path().join("messages.db")).await;
    let repo = MessengerRepo::open_turso(messages, MessengerConfig::new(OWNER))
        .await
        .expect("a messenger");
    let guard_store = store(dir.path().join("guard.db")).await;
    let guard_repo = Arc::new(TursoGuardRepository::open(guard_store).expect("encrypted"));
    let (engine, enrolment) = GuardEngine::open(GuardConfig::new(OWNER), guard_repo);
    engine.attach(&repo).expect("the first guard");

    let pair = KeyPair::from_seed(Seed::new([1; 32]));
    enrolment
        .enrol_owner(pair.pk.as_ref())
        .await
        .expect("first key");
    let id = engine.engine_identity().await.expect("a state");
    let reopen = proof(&pair, &id, Method::Reopen, &[], 1);
    engine.reopen(&reopen).await.expect("open");
    let invite = proof(&pair, &id, Method::Invite, &[AI.as_bytes()], 2);
    engine.invite(AI, &invite).await.expect("invited");

    let account = repo
        .add_account(AccountSetup {
            address: "models@local.invalid".into(),
            kind: "agent".into(),
            secret: String::new(),
        })
        .await
        .expect("an account");
    let repo = Arc::new(repo);
    let weak: Weak<MessengerRepo> = Arc::downgrade(&repo);
    let host: Weak<dyn Host> = weak;
    let link = ProviderLink {
        account: account.id,
        host,
        gate: engine.clone(),
    };

    // Port 9 is never asked. The model's name is the program's to choose; this one means nothing.
    let endpoint = Endpoint::parse("http://127.0.0.1:9").expect("loopback");
    let config = ModelConfig::new(AI, "a-name-from-your-configuration", endpoint).expect("names");
    let provider = Arc::new(
        ModelProvider::llama_cpp(config, link)
            .await
            .expect("attached"),
    );
    println!("where text goes: {:?}", provider.data_path());
    println!(
        "plan: {:?}",
        provider.encryption_plan(&[AI.to_string()]).await
    );
    let as_provider: Arc<dyn Provider> = provider.clone();
    repo.register_provider(account.id, as_provider);

    let mut draft = repo.compose(None).await.expect("a draft");
    draft.set_account(account.id).await.expect("an account");
    draft
        .set_recipients(vec![AI.into()], vec![], vec![])
        .await
        .expect("recipients");
    draft
        .set_text("hello".into(), "text/plain".into())
        .await
        .expect("text");
    println!("the core says: {:?}", draft.send(None).await);
}
