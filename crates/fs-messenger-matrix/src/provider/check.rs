//! The check: did this account ever join the room?
//!
//! Pattern: Adapter, part of `MatrixProvider` (the type is spread over the files of this folder):
//! `Room::messages` with a filter for the account's own membership events, read for `is_own_join`.
//! It asks, and never delivers what it reads.

use matrix_sdk::Room;

use super::MatrixProvider;
use crate::error::Result;
use crate::receive::Gap;

/// What one page of a check said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Checked {
    /// An own join is in the stretch.
    Found,
    /// None on this page; the next page starts here.
    Further(String),
    /// None, and the stretch is read to its end.
    Done,
}

impl MatrixProvider {
    /// One page of this check, filtered to `m.room.member` of the account.
    pub(super) async fn ask_check(&self, room: &Room, gap: &Gap) -> Result<Checked> {
        let _ = (room, gap);
        unimplemented!("MatrixProvider::ask_check")
    }
}
