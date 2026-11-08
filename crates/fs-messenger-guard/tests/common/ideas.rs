//! The accepted ideas of `requirements.md` (receipts, export, why plain, hold) at the state's own id.
//!
//! Pattern: none (tests).

use fs_messenger_core::{Messenger, MessengerRepo};
use fs_objects::ObjectId;

/// Every method the accepted ideas added to `Messenger`, called at `state`; none may reach the guard.
pub async fn every_idea_path(repo: &MessengerRepo, state: ObjectId) {
    let _ = repo.export_conversation(state).await;
    let _ = repo.why_not_encrypted(state).await;
    let _ = repo.receipt_choice().await;
    let _ = repo
        .set_receipt_choice(fs_messenger_core::ReceiptChoice::Shown)
        .await;
    let _ = repo
        .show_receipt(fs_messenger_core::ReceiptChoice::Shown)
        .await;
    let _ = repo.hold(state, 0, None).await;
    let _ = repo.held_stage(state).await;
    let _ = repo.send_held_now(state, 0).await;
    let _ = repo.recall_held(state).await;
    let _ = repo.release_due(u64::MAX).await;
}
