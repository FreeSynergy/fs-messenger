//! Reading and writing mail, with `mail-parser` and `mail-builder`.

mod build;
mod classify;
mod fields;

pub use build::{OUTER_SUBJECT, PayloadInput, build_outer, build_payload, subject_from_text};
pub use classify::{Shape, classify};
pub use fields::{AttachmentPart, MailFields, thread_root};
