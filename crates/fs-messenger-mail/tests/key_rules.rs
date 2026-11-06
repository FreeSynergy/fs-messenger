//! The plan, the Autocrypt rule and the key store: real keys, real store.

mod common;

use std::sync::Arc;

use common::person;
use fs_messenger_core::{Encryption, Provider};
use fs_messenger_mail::autocrypt::{Decision, KeyLearner, Offer};
use fs_messenger_mail::keys::{FsKeyStore, KeySource, KeyStore};
use fs_messenger_mail::openpgp::generate_key;
use fs_store::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, Store, TursoStore};

const PEER: &str = "peer@example.org";

async fn store() -> Arc<dyn KeyStore> {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    Arc::new(FsKeyStore::open(store, "https://local.invalid/me"))
}

#[tokio::test]
async fn the_plan_names_exactly_who_lacks_a_key_also_hidden_ones() {
    let me = person("me@example.org").await;
    let known = generate_key(PEER).unwrap().public();
    me.provider.import_peer_key(PEER, &known).await.unwrap();
    let plan = me
        .provider
        .encryption_plan(&[PEER.to_string()])
        .await
        .unwrap();
    assert_eq!(plan.encryption, Encryption::Openpgp);
    assert_eq!(plan.missing_keys, Vec::<String>::new());
    let who = [
        "PEER@example.org".to_string(),
        "gap@example.org".to_string(),
    ];
    let plan = me.provider.encryption_plan(&who).await.unwrap();
    assert_eq!(plan.encryption, Encryption::None);
    assert_eq!(plan.missing_keys, ["gap@example.org"]);
}

#[tokio::test]
async fn without_an_own_key_the_plan_names_the_sender() {
    let store = store().await;
    let wire = Arc::new(common::Wire::default());
    let provider = fs_messenger_mail::MailProvider::new(
        fs_messenger_mail::MailIdentity::new("me@example.org"),
        wire.clone(),
        wire,
        store,
    );
    let plan = provider.encryption_plan(&[]).await.unwrap();
    assert_eq!(plan.missing_keys, ["me@example.org"]);
}

#[tokio::test]
async fn first_seen_wins_and_a_forged_key_only_becomes_a_visible_candidate() {
    let keys = store().await;
    let learner = KeyLearner::new(keys.clone());
    let (good, evil) = (
        generate_key(PEER).unwrap().public(),
        generate_key(PEER).unwrap().public(),
    );
    let offer = |cert: &fs_messenger_mail::openpgp::PublicCert, proven| Offer {
        cert: cert.clone(),
        proven_by_known_key: proven,
    };
    assert_eq!(
        learner.learn(PEER, &offer(&good, false)).await.unwrap(),
        Decision::FirstSeen
    );
    assert_eq!(
        learner.learn(PEER, &offer(&evil, false)).await.unwrap(),
        Decision::Conflict
    );
    let held = keys.peer(PEER).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, good.fingerprint(), "the known key stays");
    assert_eq!(held.candidate.unwrap().fingerprint, evil.fingerprint());
    // The same key again is no change, and does not clear the conflict.
    let same = learner.learn(PEER, &offer(&good, false)).await.unwrap();
    assert_eq!(
        same,
        Decision::SameKey {
            take_offered: false
        }
    );
    assert!(keys.peer(PEER).await.unwrap().unwrap().candidate.is_some());
    // A person decides.
    let checked = learner.accept_candidate(PEER, &good.fingerprint()).await;
    assert!(checked.is_err(), "not the key the person compared");
    assert_eq!(
        keys.peer(PEER).await.unwrap().unwrap().fingerprint,
        good.fingerprint()
    );
    assert!(
        learner
            .accept_candidate(PEER, &evil.fingerprint())
            .await
            .unwrap()
    );
    let held = keys.peer(PEER).await.unwrap().unwrap();
    assert_eq!(held.fingerprint, evil.fingerprint());
    assert_eq!(held.source, KeySource::Person);
    assert!(
        !learner
            .accept_candidate(PEER, &evil.fingerprint())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_proof_rotates_and_a_stale_offer_cannot_turn_it_back() {
    let keys = store().await;
    let learner = KeyLearner::new(keys.clone());
    let (old, new) = (
        generate_key(PEER).unwrap().public(),
        generate_key(PEER).unwrap().public(),
    );
    let offer = |cert: &fs_messenger_mail::openpgp::PublicCert, proven| Offer {
        cert: cert.clone(),
        proven_by_known_key: proven,
    };
    learner.learn(PEER, &offer(&old, false)).await.unwrap();
    assert_eq!(
        learner.learn(PEER, &offer(&new, true)).await.unwrap(),
        Decision::Rotate
    );
    assert_eq!(
        keys.peer(PEER).await.unwrap().unwrap().source,
        KeySource::AutocryptRotated
    );
    assert_eq!(
        learner.learn(PEER, &offer(&old, false)).await.unwrap(),
        Decision::Conflict
    );
    assert_eq!(
        keys.peer(PEER).await.unwrap().unwrap().fingerprint,
        new.fingerprint()
    );
}

#[tokio::test]
async fn keys_do_not_go_into_a_plain_store_and_survive_a_restart_in_an_encrypted_one() {
    let dir = tempfile::tempdir().unwrap();
    let plain = Arc::new(TursoStore::open(dir.path().join("plain.db")).await.unwrap());
    assert!(
        FsKeyStore::open_turso(plain.clone(), "me").is_err(),
        "refused outright"
    );
    let secret = generate_key("me@example.org").unwrap();
    let behind_trait = FsKeyStore::open(plain, "me");
    assert!(
        behind_trait
            .put_own_key("me@example.org", &secret)
            .await
            .is_err(),
        "sensitive objects"
    );

    let path = dir.path().join("enc.db");
    let open = || OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    };
    {
        let enc = Arc::new(TursoStore::open_with(&path, open()).await.unwrap());
        let keys = FsKeyStore::open_turso(enc, "me").unwrap();
        keys.put_own_key("Me@Example.org", &secret).await.unwrap();
    }
    let enc = Arc::new(TursoStore::open_with(&path, open()).await.unwrap());
    let keys = FsKeyStore::open_turso(enc, "me").unwrap();
    let back = keys.own_key("me@example.org").await.unwrap().unwrap();
    assert_eq!(back.fingerprint(), secret.fingerprint());
    // No secret in its Debug, and a replacement keeps one record.
    assert!(format!("{back:?}").contains("redacted"));
    let other = generate_key("me@example.org").unwrap();
    keys.put_own_key("me@example.org", &other).await.unwrap();
    assert_eq!(
        keys.own_key("me@example.org")
            .await
            .unwrap()
            .unwrap()
            .fingerprint(),
        other.fingerprint()
    );
}

/// fs-store logs before and after images and offers no way around it, so an own key ends up in the
/// activity log and in `Debug` of an activity, encrypted at rest with the store. Pinned here, because
/// DESIGN.md states it: when fs-store changes this, this test says so. Wanted from fs-store.
#[tokio::test]
async fn known_limit_the_own_key_is_in_the_activity_log_of_its_store() {
    let store = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let keys = FsKeyStore::open(store.clone(), "me");
    keys.put_own_key("me@example.org", &generate_key("me@example.org").unwrap())
        .await
        .unwrap();
    let log = Store::activities(store.as_ref(), &fs_store::ActivityFilter::default())
        .await
        .unwrap();
    assert!(
        format!("{log:?}").contains("secret"),
        "the limit is gone: update DESIGN.md"
    );
}
