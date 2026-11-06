//! Cursor paging over store queries.
//!
//! Pattern: Iterator (by hand): fetch one more than a page, the extra one says there is a next.

use fs_store::StoredObject;

use crate::model::Cursor;

/// The page's objects and the cursor to the next page.
pub(super) fn split_page(
    mut objects: Vec<StoredObject>,
    size: usize,
    key: &str,
) -> (Vec<StoredObject>, Option<Cursor>) {
    let more = objects.len() > size;
    objects.truncate(size);
    let next = more
        .then(|| objects.last())
        .flatten()
        .and_then(|o| o.index.get(key))
        .and_then(|values| values.first())
        .map(|k| Cursor::new(k.clone()));
    (objects, next)
}
