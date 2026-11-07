//! Tests against a real homeserver, the pinned tuwunel, with real Olm and Megolm. They are `#[ignore]`d.
//! Run:
//!
//!     bash crates/fs-messenger-matrix/tests/server/own-server.sh up
//!     FS_MSGTEST_MATRIX_URL=http://127.0.0.1:18108 \
//!       cargo test -j 4 -p fs-messenger-matrix --test server -- --ignored
//!     bash crates/fs-messenger-matrix/tests/server/own-server.sh down
//!
//! `scripts/test-servers/up.sh` works too: without the variable the tests read `env.sh`. Each test
//! registers its own users, so they run side by side and a server can be used again.
//!
//! No test double anywhere in this binary: real provider, real core on an encrypted `TursoStore`, real
//! server. What the server holds is read by plain HTTP, past the provider.

mod acts;
mod capture;
mod common;
mod http;
mod rooms;

mod banned;
mod ciphertext;
mod kicked;
mod late_key;
mod membership;
mod membership_acts;
mod plain;
mod rejoin;
mod replay;
mod restart;
mod roundtrip;
mod secrets;
