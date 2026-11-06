//! The store-backed implementation of the contract.

mod accounts;
mod actions;
mod api;
mod attachments;
mod capabilities;
mod consts;
mod conversation_codec;
mod conversations;
mod deletion;
mod draft_data;
mod draft_handle;
mod feed;
mod forward;
mod handle;
mod ingest;
mod ingest_store;
mod keys;
mod message_codec;
mod messages;
mod paging;
mod search;
mod send;
mod send_record;
mod store_check;
mod subscription;
mod text;
mod tool_calls;

pub use handle::{MessengerConfig, MessengerRepo};
