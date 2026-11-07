//! The whole way once: first key, reopen, invite, a question, a decision, the stop.
//!
//! Run: `cargo run -p fs-messenger-guard --example guard_walkthrough`. It generates a device key in this
//! process only because it is an example; a real private key stays on the person's device.

use std::sync::Arc;

use ed25519_compact::{KeyPair, Seed};
use fs_messenger_core::{
    MessageAction, Messenger, MessengerConfig, MessengerRepo, ToolCallDecision,
};
use fs_messenger_guard::{
    Arguments, Caller, Gate, Guard, GuardConfig, GuardEngine, GuardProof, Method,
    TursoGuardRepository, Verdict, decide_arguments, signed_bytes,
};
use fs_objects::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};

const OWNER: &str = "https://local.invalid/users/owner";
const AI: &str = "https://local.invalid/ai/helper";

fn proof(pair: &KeyPair, engine: &[u8], method: Method, args: &[&[u8]], n: u8) -> GuardProof {
    let nonce = vec![n; 16];
    let bytes = signed_bytes(engine, method, OWNER, args, &nonce).expect("short fields");
    GuardProof {
        signer: OWNER.into(),
        nonce,
        proof: pair.sk.sign(&bytes, None).to_vec(),
    }
}

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

#[tokio::main]
async fn main() {
    let dir = tempfile::tempdir().expect("a folder");
    let messages = store(dir.path().join("messages.db")).await;
    let repo = MessengerRepo::open_turso(messages, MessengerConfig::new(OWNER))
        .await
        .expect("a messenger");
    // A store of its own for the guard.
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
    let gate: Arc<dyn Gate> = engine.clone();

    let args = Arguments::from_text(r#"{"message":"m1"}"#).expect("JSON");
    println!(
        "fresh: {:?}",
        gate.check(&Caller::new(AI), "message.delete", &args).await
    );

    engine
        .reopen(&proof(&pair, &id, Method::Reopen, &[], 1))
        .await
        .expect("open");
    let invite = proof(&pair, &id, Method::Invite, &[AI.as_bytes()], 2);
    engine.invite(AI, &invite).await.expect("invited");

    let Verdict::Ask(q) = gate.check(&Caller::new(AI), "message.delete", &args).await else {
        panic!("destroying asks");
    };
    println!(
        "asks: {} {:?}, risk {:?}",
        q.call.name, q.call.state, q.call.risk
    );

    let (message, revision) = (q.message.to_string(), q.call.revision.to_be_bytes());
    let fields = decide_arguments(
        &message,
        &q.call.id,
        "allow-once",
        &q.call.arguments_digest,
        &revision,
    );
    let bytes = signed_bytes(&id, Method::Decide, OWNER, &fields, &[]).expect("short fields");
    let decision = ToolCallDecision {
        tool_call: q.call.id.clone(),
        option: "allow-once".into(),
        arguments_digest: q.call.arguments_digest.clone(),
        proof: pair.sk.sign(&bytes, None).to_vec(),
        signer: OWNER.into(),
        revision: q.call.revision,
    };
    repo.act(q.conversation, q.message, MessageAction::Decide(decision))
        .await
        .expect("decided");
    println!(
        "once: {:?}",
        gate.check(&Caller::new(AI), "message.delete", &args).await
    );

    let report = engine
        .stop(&proof(&pair, &id, Method::Stop, &[], 3))
        .await
        .expect("stopped");
    println!("stop: {report:?}");
    println!(
        "after: {:?}",
        gate.check(&Caller::new(AI), "message.delete", &args).await
    );
    for entry in engine.log().await.expect("the log") {
        println!("log: {} {}", entry.event, entry.profile);
    }
}
