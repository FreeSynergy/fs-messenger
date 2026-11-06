//! A legal `|` in a `Message-ID` must not cost the mail or any reply (ADR 0030). Through the real core.

mod common;

use common::{core_with, person, wire_in};
use fs_messenger_core::{Ingest, Messenger, SearchFilter};
use fs_messenger_mail::openpgp::generate_key;

const BOB: &str = "b@example.org";

#[tokio::test]
async fn it_is_stored_threads_and_a_reply_cites_the_id_as_it_was_written() {
    let bob = person(BOB).await;
    let (repo, account) = core_with(&bob).await;
    let from = "a@example.org";
    wire_in(
        &bob,
        format!("From: {from}\r\nSubject: s\r\nMessage-ID: <r|oot@e>\r\n\r\nfirst\r\n"),
    );
    wire_in(
        &bob,
        format!(
            "From: {from}\r\nSubject: s\r\nMessage-ID: <c%hild@e>\r\nIn-Reply-To: <r|oot@e>\r\n\
References: <r|oot@e>\r\n\r\nsecond\r\n"
        ),
    );
    let report = repo.sync(account).await.unwrap();
    assert_eq!((report.stored, report.rejected), (2, 0), "{report:?}");

    let page = repo
        .search("second", &SearchFilter::default(), None)
        .await
        .unwrap();
    let summary = &page.items[0];
    let conversation = summary
        .envelope
        .context
        .as_deref()
        .unwrap()
        .parse()
        .unwrap();
    let child = repo
        .message(conversation, summary.envelope.id)
        .await
        .unwrap();
    let first = repo
        .search("first", &SearchFilter::default(), None)
        .await
        .unwrap();
    assert_eq!(
        child.summary.envelope.in_reply_to,
        Some(first.items[0].envelope.id.to_string())
    );
    assert_eq!(
        first.items[0].envelope.context, summary.envelope.context,
        "one thread"
    );

    bob.provider
        .import_peer_key(from, &generate_key(from).unwrap().public())
        .await
        .unwrap();
    let mut reply = repo.compose(Some(child.id())).await.unwrap();
    reply
        .set_recipients(vec![from.into()], vec![], vec![])
        .await
        .unwrap();
    reply.set_subject("Re".into()).await.unwrap();
    reply
        .set_text("third".into(), "text/plain".into())
        .await
        .unwrap();
    reply.send(None).await.unwrap();

    let sent = bob.wire.sent.lock().unwrap();
    let text = String::from_utf8_lossy(&sent[0].1);
    assert!(text.contains("In-Reply-To: <c%hild@e>"), "{text}");
    assert!(text.contains("<r|oot@e>"), "the chain as written: {text}");
}
