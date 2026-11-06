//! Tests against a real mail server, Stalwart from `scripts/test-servers`. They are `#[ignore]`d. Run:
//!
//!     bash scripts/test-servers/down.sh && bash scripts/test-servers/up.sh
//!     cargo test -j 8 -p fs-messenger-mail --test server -- --ignored
//!     bash scripts/test-servers/down.sh
//!
//! A fresh server each time, for a state nobody touched. `env::relax_throttle` switches Stalwart's default
//! throttle off at runtime: it allows 25 mails per hour per sender domain and recipient, a full run sends
//! more, and past it SMTP answers 452 and JMAP submissions wait.
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
mod wire;
