//! Search, and drafts that are kept, resumed and discarded.

mod common;

use common::{account, at, raw, setup, with_provider};
use fs_messenger_core::{Encryption, Error, Ingest, Messenger, RawAttachment, SearchFilter};
use fs_store::Query;
use fs_store::Store;

#[tokio::test]
async fn search_finds_subject_and_text_and_filters_narrow_it() {
    let (repo, _) = setup().await;
    let me = account(&repo, "me@example.org").await;
    let mut a = raw("a", "t", "the Quarterly report");
    a.published = Some(at(1));
    let mut b = raw("b", "t", "nothing here");
    b.subject = Some("quarterly numbers".into());
    b.sender = "bob@example.org".into();
    b.published = Some(at(2));
    b.attachments.push(RawAttachment {
        name: "x".into(),
        media_type: "text/plain".into(),
        data: vec![1],
    });
    let mut c = raw("c", "t", "unrelated");
    c.published = Some(at(3));
    for m in [a, b, c] {
        repo.ingest(me.id, m).await.unwrap();
    }
    let none = SearchFilter::default();
    let hits = repo.search("QUARTERLY", &none, None).await.unwrap();
    let got: Vec<_> = hits
        .items
        .iter()
        .map(|m| m.envelope.name.as_str())
        .collect();
    assert_eq!(
        got,
        ["quarterly numbers", "the Quarterly report"],
        "newest first"
    );

    let from = SearchFilter {
        from: Some("bob".into()),
        ..SearchFilter::default()
    };
    assert_eq!(
        repo.search("quarterly", &from, None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    let files = SearchFilter {
        has_attachment: true,
        ..SearchFilter::default()
    };
    assert_eq!(repo.search("", &files, None).await.unwrap().items.len(), 1);
    let since = SearchFilter {
        since: Some(at(3)),
        ..SearchFilter::default()
    };
    assert_eq!(
        repo.search("", &since, None).await.unwrap().items[0].preview,
        "unrelated"
    );
    let unread = SearchFilter {
        unread: true,
        ..SearchFilter::default()
    };
    assert_eq!(repo.search("", &unread, None).await.unwrap().items.len(), 3);
    assert!(
        repo.search("100%", &none, None)
            .await
            .unwrap()
            .items
            .is_empty(),
        "% is literal"
    );
}

#[tokio::test]
async fn search_pages_with_a_cursor() {
    let store: std::sync::Arc<dyn Store> =
        std::sync::Arc::new(fs_store::TursoStore::open_in_memory().await.unwrap());
    let repo = common::repo_over(store, 2).await;
    let me = account(&repo, "me@example.org").await;
    for n in 0..3 {
        repo.ingest(me.id, common::raw_at(&format!("find{n}"), "t", n))
            .await
            .unwrap();
    }
    let none = SearchFilter::default();
    let first = repo.search("find", &none, None).await.unwrap();
    let second = repo
        .search("find", &none, first.next.as_ref())
        .await
        .unwrap();
    assert_eq!((first.items.len(), second.items.len()), (2, 1));
    assert_eq!(second.items[0].preview, "find0");
    assert_eq!(second.next, None);
}

#[tokio::test]
async fn a_draft_is_kept_resumed_and_never_among_the_messages() {
    let (repo, store) = setup().await;
    let (_, provider) = with_provider(&repo, Encryption::Mls, &["bob@example.org"]).await;
    let mut d = repo.compose(None).await.unwrap();
    d.set_recipients(vec!["bob@example.org".into()], vec![], vec![])
        .await
        .unwrap();
    d.set_text("half done".into(), "text/plain".into())
        .await
        .unwrap();
    let id = d.save().await.unwrap();
    assert_eq!(id.as_uuid().get_version_num(), 7);
    assert_eq!(repo.drafts().await.unwrap()[0].text, "half done");
    assert_eq!(store.count(&Query::new().of_type("Note")).await.unwrap(), 0);
    assert!(
        repo.search("half", &SearchFilter::default(), None)
            .await
            .unwrap()
            .items
            .is_empty()
    );

    let mut again = repo.resume_draft(id).await.unwrap();
    again
        .attach("a.txt".into(), "text/plain".into(), b"file".to_vec())
        .await
        .unwrap();
    let sent = again.send(None).await.unwrap();
    assert!(repo.drafts().await.unwrap().is_empty());
    assert_eq!(
        provider.sent.lock().unwrap()[0].attachments()[0].data,
        b"file"
    );
    let conv = common::conversation_of(&repo).await;
    let m = repo.message(conv, sent).await.unwrap();
    assert_eq!(
        repo.attachment(m.attachments[0].blob).await.unwrap().bytes,
        b"file"
    );
}

#[tokio::test]
async fn discard_removes_the_draft_and_its_files_and_ends_it() {
    let (repo, store) = setup().await;
    account(&repo, "me@example.org").await;
    let mut d = repo.compose(None).await.unwrap();
    d.attach("a".into(), String::new(), vec![1]).await.unwrap();
    assert_eq!(
        store
            .count(&Query::new().of_type("Document"))
            .await
            .unwrap(),
        1
    );
    d.discard().await.unwrap();
    assert!(repo.drafts().await.unwrap().is_empty());
    assert_eq!(
        store
            .count(&Query::new().of_type("Document"))
            .await
            .unwrap(),
        0
    );
    assert!(matches!(d.save().await, Err(Error::Conflict(_))));
}

#[tokio::test]
async fn only_the_simple_editors_formats_are_taken() {
    let (repo, _) = setup().await;
    let mut d = repo.compose(None).await.unwrap();
    let bad = d.set_text("<b>x</b>".into(), "text/html".into()).await;
    assert!(matches!(bad, Err(Error::Invalid(_))));
}
