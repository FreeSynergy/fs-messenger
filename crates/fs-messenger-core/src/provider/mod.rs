//! What a connector offers (ADR 0045); real connectors live in their own crates.

mod outgoing;
mod raw;
mod traits;

pub(crate) use outgoing::OutgoingBody;
pub use outgoing::{Outgoing, OutgoingAttachment, SendReceipt};
pub use raw::{FetchBatch, ProviderCursor, RawAttachment, RawMessage, RawToolCall};
pub use traits::Provider;
