//! The journal over a long run: folding keeps what counts, and journals stay apart.

mod support;

use fs_messenger_matrix::store::{FsJournal, Journal, Op};
use fs_store_unlogged::Query;
use fs_store_unlogged::traits::ObjectQuery;

#[tokio::test]
async fn a_long_journal_folds_and_loses_nothing() {
    let store = support::fresh_store().await;
    let journal = FsJournal::open(store.clone(), "long").await.unwrap();
    let neighbour = FsJournal::open(store.clone(), "neighbour").await.unwrap();
    neighbour
        .append(vec![Op::Put("mine".into(), "1".into())])
        .await
        .unwrap();

    for i in 0..600 {
        let ops = vec![
            Op::Put(format!("k{i}"), i.to_string()),
            Op::Put("last".into(), i.to_string()),
        ];
        journal.append(ops).await.unwrap();
        if i % 2 == 1 {
            journal
                .append(vec![Op::Remove(format!("k{}", i - 1))])
                .await
                .unwrap();
        }
    }
    let map = journal.load().await.unwrap();
    assert_eq!(map["last"], "599");
    assert_eq!(map.len(), 301, "300 odd keys and `last`");
    assert!(!map.contains_key("k0"));

    let q = Query::new().of_type("freesynergy.net:MatrixStoreSegment");
    let segments = store.query(&q).await.unwrap();
    assert!(segments.len() < 900, "folded: {} objects", segments.len());

    // After the fold, a new handle reads the same and goes on numbering after it.
    drop(journal);
    let journal = FsJournal::open(store.clone(), "long").await.unwrap();
    journal
        .append(vec![Op::Put("later".into(), "x".into())])
        .await
        .unwrap();
    let map = journal.load().await.unwrap();
    assert_eq!(map["last"], "599");
    assert_eq!(map["later"], "x");
    assert_eq!(neighbour.load().await.unwrap().len(), 1);
}
