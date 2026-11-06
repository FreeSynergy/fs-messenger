//! Tests against a real mail server, Stalwart from `scripts/test-servers`. They are `#[ignore]`d. Run in two
//! batches, each on a fresh server (`down.sh`, then `up.sh`):
//!
//!     cargo test -j 4 -p fs-messenger-mail --test server -- --ignored encrypted needles plain refused autocrypt
//!     cargo test -j 4 -p fs-messenger-mail --test server -- --ignored replay paging secrets
//!
//! Two, because Stalwart's default throttle allows 25 mails per hour per sender domain and recipient, and
//! all of them together send more. Past it, SMTP answers 452 and JMAP submissions wait: that is the
//! throttle, not a finding. Setting it off through the API has no effect before a reload.
//!
//! One binary, because the counts in these tests need one test at a time on the shared mailboxes; each
//! test holds the lock of `common::serial`.

mod autocrypt;
mod common;
mod encrypted;
mod env;
#[path = "../common/forge.rs"]
mod forge;
mod needles;
mod paging;
mod plain;
mod refused;
mod replay;
mod secrets;
mod wire;
