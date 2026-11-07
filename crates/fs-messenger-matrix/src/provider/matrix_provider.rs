//! [`MatrixProvider`]: one Matrix account behind the core's `Provider` trait.
//!
//! Pattern: Adapter: the struct wraps the SDK's `Client` and implements the core's `Provider` over it.
//! What it cannot do answers `Unsupported`, through the trait's defaults and in `capabilities`. The
//! type is spread over the files of this folder (`fetch`, `send`, `convert`, `history`, `connect`,
//! `rooms`, `plan`); each says `Adapter, part of MatrixProvider`.

use async_trait::async_trait;
use fs_interfaces::provider::Capabilities;
use fs_messenger_core::{
    EncryptionPlan, FetchBatch, Outgoing, Provider, ProviderCursor, Result as CoreResult,
    SendReceipt,
};
use matrix_sdk::Client;
use tokio::sync::Mutex;

use super::MatrixAccount;
use super::echo::SentEvents;
use crate::error::{MatrixError, Result};

/// The provider of one account. Keep one per store: the stores are not shared between processes.
pub struct MatrixProvider {
    pub(super) account: MatrixAccount,
    pub(super) client: Client,
    /// Sending and syncing exclude each other; see `echo.rs`.
    pub(super) sent: Mutex<SentEvents>,
}

impl std::fmt::Debug for MatrixProvider {
    // Only ids. The client holds the access token, so it is not asked to print itself.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixProvider")
            .field("user", &self.account.user_id)
            .field("device", &self.client.device_id())
            .finish_non_exhaustive()
    }
}

impl MatrixProvider {
    pub(super) async fn started(account: MatrixAccount, client: Client) -> Result<Self> {
        // The SDK's start-up task goes to the network (secret storage in the account data), so it
        // has the same hard end as every other call. Past it, the provider is not handed out.
        let encryption = client.encryption();
        let ready = async {
            encryption.wait_for_e2ee_initialization_tasks().await;
            Ok::<(), MatrixError>(())
        };
        account.bounded(ready).await?;
        Ok(Self {
            account,
            client,
            sent: Mutex::new(SentEvents::default()),
        })
    }

    /// The Matrix id, which is the account's address in the core.
    pub fn user_id(&self) -> &str {
        &self.account.user_id
    }

    /// This device's id. Stays the same across a restart.
    pub fn device_id(&self) -> Result<String> {
        let id = self.client.device_id();
        id.map(ToString::to_string)
            .ok_or_else(|| MatrixError::Sdk("not logged in".into()))
    }

    /// This device's public signing key. Stays the same across a restart.
    pub async fn identity_key(&self) -> Option<String> {
        self.client.encryption().ed25519_key().await
    }
}

#[async_trait]
impl Provider for MatrixProvider {
    fn kind(&self) -> CoreResult<String> {
        Ok("matrix".to_owned())
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::new("fs-messenger-matrix");
        for id in [
            "provider.kind",
            "message.fetch",
            "message.send",
            "encryption.plan",
        ] {
            caps.supported(id);
        }
        for (id, why) in [
            ("message.push", "no long polling yet; call fetch"),
            ("message.bcc", "a room has no hidden members"),
            ("attachment.send", "encrypted attachments are not built"),
            ("attachment.fetch", "files show by name; not downloaded"),
            ("key.verify", "no device verification; see DESIGN.md"),
            ("device.list", "not offered; see DESIGN.md"),
            ("key.backup", "no server-side key backup"),
            ("presence", "not built; the sync says offline"),
            ("call", "not built"),
            ("bridge", "not built"),
        ] {
            caps.unsupported(id, why);
        }
        caps
    }

    async fn fetch(&self, since: Option<&ProviderCursor>) -> CoreResult<FetchBatch> {
        Ok(self.fetch_batch(since).await?)
    }

    async fn send(&self, message: &Outgoing) -> CoreResult<SendReceipt> {
        self.account
            .whole(Box::pin(self.send_message(message)))
            .await
    }

    async fn encryption_plan(&self, recipients: &[String]) -> CoreResult<EncryptionPlan> {
        let plan = async { Ok(self.plan_for(recipients).await?) };
        self.account.whole(Box::pin(plan)).await
    }
}
