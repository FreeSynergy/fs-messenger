//! Tests against a real mail server, Stalwart from `scripts/test-servers`. They are `#[ignore]`d. Run:
//!
//!     bash scripts/test-servers/down.sh && bash scripts/test-servers/up.sh
//!     cargo test -j 8 -p fs-messenger-mail --test server -- --ignored
//!     bash scripts/test-servers/down.sh
//!
//! A fresh server each time: a full run sends about 25 mails, and Stalwart's default throttle allows 25
//! per hour per sender domain and recipient, after which SMTP answers 452 and JMAP submissions wait.
//!
//! One binary, because the counts in these tests need one test at a time on the shared mailboxes; each
//! test holds the lock of `common::serial`.

mod autocrypt;
mod common;
mod encrypted;
mod env;
mod needles;
mod plain;
mod refused;
mod replay;
mod wire;
