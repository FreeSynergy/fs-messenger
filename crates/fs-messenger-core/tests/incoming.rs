//! ADR 0018, incoming: unencrypted is stored and marked, never quarantined; `unknown` never reads as
//! encrypted.

mod common;

use common::{account, conversation_of, raw, setup};
use fs_messenger_core::{Encryption, Ingest, Ingested, Messenger, RawMessage};

async fn delivered(encryption: Option<Encryption>) -> (Encryption, Encryption, bool) {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut m: RawMessage = raw("m1", "t", "plain text");
    match encryption {
        Some(e) => m.encryption = e,
        None => {
            m.encryption =
                RawMessage::new(common::alias("a"), common::alias("c"), "s", "t").encryption
        }
    }
    let Ingested::Stored(id) = repo.ingest(me.id, m).await.unwrap() else {
        panic!();
    };
    let conv = conversation_of(&repo).await;
    let shown = repo.message(conv, id).await.unwrap().summary;
    assert!(shown.unread, "shown like any other message");
    let listed = repo
        .conversations(None, false, None)
        .await
        .unwrap()
        .items
        .remove(0);
    (
        shown.encryption,
        listed.encryption,
        shown.encryption.is_end_to_end(),
    )
}

#[tokio::test]
async fn unencrypted_is_stored_and_marked() {
    assert_eq!(
        delivered(Some(Encryption::None)).await,
        (Encryption::None, Encryption::None, false)
    );
    assert_eq!(
        delivered(Some(Encryption::TransportOnly)).await,
        (Encryption::TransportOnly, Encryption::TransportOnly, false)
    );
}

#[tokio::test]
async fn a_provider_that_says_nothing_gives_unknown_never_encrypted() {
    assert_eq!(
        delivered(None).await,
        (Encryption::Unknown, Encryption::Unknown, false)
    );
}
