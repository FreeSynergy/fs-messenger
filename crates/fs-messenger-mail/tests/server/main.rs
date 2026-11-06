//! Tests against a real mail server, Stalwart from `scripts/test-servers`. They are `#[ignore]`d. Run:
//!
//!     bash scripts/test-servers/up.sh
//!     cargo test -j 8 -p fs-messenger-mail --test server -- --ignored
//!     bash scripts/test-servers/down.sh
//!
//! One binary, because the counts in these tests need one test at a time on the shared mailboxes; each
//! test holds the lock of `common::serial`.

mod autocrypt;
mod common;
mod encrypted;
mod needles;
mod plain;
mod refused;
mod replay;
