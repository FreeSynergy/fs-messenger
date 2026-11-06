//! Reading one IMAP mailbox: what comes after the cursor, within the limits.
//!
//! Pattern: Template method: search, ask the sizes, take what the budget allows, read. Opened read-only
//! (`EXAMINE`, `BODY.PEEK`), so fetching marks nothing as read. The cursor moves past what was taken and
//! no further. Of a mail over the limit only the headers' worth of bytes is read.

use std::collections::{HashMap, HashSet};

use async_imap::Session;
use futures_util::TryStreamExt;

use super::imap::{Stream, err};
use super::{
    Budget, FetchLimits, FetchedBatch, FetchedMail, HEAD_BYTES, MailboxCursor, MailboxRole,
};
use crate::error::Result;

/// The `UID SEARCH` for what comes after `after`; nothing can come after the highest UID.
fn uids_after(after: u32) -> Option<String> {
    after.checked_add(1).map(|next| format!("UID {next}:*"))
}

fn set_of(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// The sizes the server reports, by UID.
async fn sizes(session: &mut Session<Stream>, uids: &[u32]) -> Result<HashMap<u32, u32>> {
    let fetched: Vec<_> = session
        .uid_fetch(set_of(uids), "(UID RFC822.SIZE)")
        .await
        .map_err(err)?
        .try_collect()
        .await
        .map_err(err)?;
    Ok(fetched
        .iter()
        .filter_map(|f| Some((f.uid?, f.size?)))
        .collect())
}

/// Where the mails come from.
#[derive(Clone, Copy)]
struct Origin<'a> {
    name: &'a str,
    uid_validity: u32,
    role: MailboxRole,
}

async fn read_bodies(
    session: &mut Session<Stream>,
    origin: Origin<'_>,
    uids: &[u32],
    section: &str,
    sizes_over: Option<&HashMap<u32, u32>>,
    batch: &mut FetchedBatch,
) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    let fetched: Vec<_> = session
        .uid_fetch(set_of(uids), format!("(UID {section})"))
        .await
        .map_err(err)?
        .try_collect()
        .await
        .map_err(err)?;
    let mut mails: Vec<(u32, Vec<u8>)> = fetched
        .iter()
        .filter_map(|f| Some((f.uid?, f.body()?.to_vec())))
        .collect();
    mails.sort_unstable_by_key(|(uid, _)| *uid);
    if mails.len() != uids.len() {
        return Err(err(format!(
            "{}: asked for {} mails, got {}; the cursor stays",
            origin.name,
            uids.len(),
            mails.len()
        )));
    }
    for (uid, bytes) in mails {
        batch.mails.push(FetchedMail {
            bytes,
            role: origin.role,
            stable_id: format!("{}/{}/{uid}", origin.name, origin.uid_validity),
            too_large: sizes_over.and_then(|s| s.get(&uid)).map(|s| u64::from(*s)),
        });
    }
    Ok(())
}

/// One mailbox, as far as the limits and the budget go.
pub(super) async fn read_mailbox(
    session: &mut Session<Stream>,
    (name, role): (&str, MailboxRole),
    limits: &FetchLimits,
    budget: &mut Budget,
    batch: &mut FetchedBatch,
) -> Result<()> {
    let mailbox = session.examine(name).await.map_err(err)?;
    let uid_validity = mailbox.uid_validity.unwrap_or(0);
    let after = match batch.cursor.mailboxes.get(name) {
        Some(MailboxCursor::Imap {
            uid_validity: held,
            last_uid,
        }) if *held == uid_validity => *last_uid,
        _ => 0,
    };
    // `n:*` always matches the newest mail, even if its UID is below `n`.
    let found = match uids_after(after) {
        Some(search) => session.uid_search(search).await.map_err(err)?,
        None => HashSet::new(),
    };
    let mut uids: Vec<u32> = found.into_iter().filter(|u| *u > after).collect();
    uids.sort_unstable();
    let mut more = uids.len() > limits.mails;
    uids.truncate(limits.mails);
    let known = if uids.is_empty() {
        HashMap::new()
    } else {
        sizes(session, &uids).await?
    };
    let bytes = u64::try_from(limits.bytes).unwrap_or(u64::MAX);
    let (mut fitting, mut large) = (vec![], vec![]);
    let mut last_uid = after;
    for uid in uids {
        let size = known.get(&uid).map_or(0, |s| u64::from(*s));
        let over = size > bytes;
        if !budget.admit(if over { HEAD_BYTES } else { size }) {
            more = true;
            break;
        }
        last_uid = uid;
        if over {
            large.push(uid);
        } else {
            fitting.push(uid);
        }
    }
    let origin = Origin {
        name,
        uid_validity,
        role,
    };
    read_bodies(session, origin, &fitting, "BODY.PEEK[]", None, batch).await?;
    let head = format!("BODY.PEEK[]<0.{HEAD_BYTES}>");
    read_bodies(session, origin, &large, &head, Some(&known), batch).await?;
    batch.more |= more;
    batch.cursor.mailboxes.insert(
        name.to_string(),
        MailboxCursor::Imap {
            uid_validity,
            last_uid,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_comes_after_the_highest_uid() {
        assert_eq!(uids_after(5).as_deref(), Some("UID 6:*"));
        assert_eq!(uids_after(u32::MAX), None);
    }
}
