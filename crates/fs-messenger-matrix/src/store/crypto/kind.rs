//! The record kinds of the crypto journal.
//!
//! Pattern: none (constants): one name per thing the store keeps.

pub(super) const ACCOUNT: &str = "account";
pub(super) const IDENTITY: &str = "identity";
pub(super) const BACKUP_VERSION: &str = "backup_version";
pub(super) const BACKUP_KEY: &str = "backup_key";
pub(super) const DEHYDRATED: &str = "dehydrated";
pub(super) const SESSION: &str = "session";
pub(super) const HASH: &str = "hash";
pub(super) const INBOUND: &str = "inbound";
pub(super) const INBOUND_BACKED_UP: &str = "inbound_backed_up";
pub(super) const OUTBOUND: &str = "outbound";
pub(super) const REQUEST: &str = "request";
pub(super) const USER_IDENTITY: &str = "user_identity";
pub(super) const DEVICE: &str = "device";
pub(super) const WITHHELD: &str = "withheld";
pub(super) const SETTINGS: &str = "settings";
pub(super) const SECRET: &str = "secret";
pub(super) const NEXT_BATCH: &str = "next_batch";
pub(super) const BUNDLE: &str = "bundle";
pub(super) const DOWNLOADED: &str = "downloaded";
pub(super) const PENDING_BUNDLE: &str = "pending_bundle";
pub(super) const TRACKED: &str = "tracked";
pub(super) const CUSTOM: &str = "custom";
