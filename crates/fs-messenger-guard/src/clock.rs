//! [`Clock`]: what time it is for expiries.
//!
//! Pattern: Strategy: the state machine stays pure, and expiry can be tested without waiting. A clock
//! set back keeps a question alive for longer; that is the operating system's matter, see `DESIGN.md`.

use chrono::{DateTime, Utc};

/// Now.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> DateTime<Utc>;
}

/// The system's clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
