//! Cursor paging over store queries.
//!
//! Pattern: Iterator (by hand): fetch one more than a page, the extra one says there is a next. The
//! cursor comes from the stored key, so a record that cannot be read cannot stop anyone going on.

use fs_store::StoredObject;

use crate::error::Result;
use crate::model::{Cursor, Page};

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

/// Decode what can be decoded, and report the rest.
pub(super) fn decoded<T>(
    objects: Vec<StoredObject>,
    next: Option<Cursor>,
    decode: impl Fn(StoredObject) -> Result<T>,
) -> Page<T> {
    let mut page = Page {
        items: Vec::with_capacity(objects.len()),
        next,
        skipped: vec![],
    };
    for o in objects {
        let id = o.envelope.id;
        match decode(o) {
            Ok(item) => page.items.push(item),
            Err(_) => page.skipped.push(id),
        }
    }
    page
}
