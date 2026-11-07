//! The check: did this account ever join the room?
//!
//! Pattern: Adapter, part of `MatrixProvider` (the type is spread over the files of this folder):
//! `Room::messages` with a filter for the account's own membership events, read for `is_own_join`.
//! It asks, and never delivers what it reads: a page is looked at and dropped.

use matrix_sdk::Room;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::{OwnedUserId, UInt};

use super::MatrixProvider;
use crate::error::{MatrixError, Result};
use crate::receive::{Gap, MatrixCursor, Why, after_check, any_own_join};

/// What one page of a check said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Checked {
    /// An own join is in the stretch.
    Found,
    /// None on this page; the next page starts at `end`. `empty`: the page held nothing.
    Further { end: String, empty: bool },
    /// None, and the stretch is read to its end.
    Done,
}

/// Events asked for per page of a check.
const PAGE: u32 = 100;

impl MatrixProvider {
    /// One page of this check, filtered to `m.room.member` of the account: the server keeps the
    /// content of others out, and nothing of this page is kept.
    pub(super) async fn ask_check(&self, room: &Room, gap: &Gap) -> Result<Checked> {
        let own = OwnedUserId::try_from(self.account.user_id.as_str())
            .map_err(|e| MatrixError::Config(e.to_string()))?;
        let mut options = MessagesOptions::backward().from(gap.from.as_str());
        options.to.clone_from(&gap.to);
        options.limit = UInt::from(PAGE);
        options.filter.types = Some(vec!["m.room.member".to_owned()]);
        options.filter.senders = Some(vec![own]);
        let page = self.account.bounded(room.messages(options)).await?;
        let jsons = page.chunk.iter().map(|e| e.raw().json().get());
        if any_own_join(&self.account.user_id, jsons) {
            return Ok(Checked::Found);
        }
        Ok(match page.end {
            Some(end) => Checked::Further {
                end,
                empty: page.chunk.is_empty(),
            },
            None => Checked::Done,
        })
    }

    /// One page of the check that is the last open gap, and what follows from it. Answers whether
    /// the page was empty. A refusal gives the check up by name; any other error passes.
    pub(super) async fn work_check(
        &self,
        cursor: &mut MatrixCursor,
        room: &Room,
        check: Gap,
    ) -> Result<bool> {
        match self.ask_check(room, &check).await {
            Ok(Checked::Found) => {
                cursor.gaps.pop();
                let (read, becomes) = after_check(&check);
                cursor.plan_gap(read);
                cursor.become_(&check.room, becomes);
                Ok(false)
            }
            Ok(Checked::Further { end, empty }) => {
                if let Some(open) = cursor.gaps.last_mut() {
                    open.from = end;
                }
                Ok(empty)
            }
            Ok(Checked::Done) => {
                cursor.gaps.pop();
                tracing::debug!(room = %check.room, "no own join in the stretch: not a room we accepted");
                Ok(false)
            }
            Err(MatrixError::Forbidden(_)) => {
                if let Some(gap) = cursor.gaps.pop() {
                    cursor.give_up(gap, Why::CheckRefused);
                }
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }
}
