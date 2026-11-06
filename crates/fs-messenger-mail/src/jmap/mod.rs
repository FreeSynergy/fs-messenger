//! JMAP (RFC 8620, 8621): one small client, and the fetch and submission strategies over it.

mod client;
mod fetch;
mod listing;
mod session;
mod submit;

pub use client::{JmapClient, JmapConfig};
pub use fetch::JmapFetch;
pub use submit::JmapSubmit;
