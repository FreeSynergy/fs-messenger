//! Accounts: stored objects, one per address and kind.
//!
//! Pattern: Repository. The sync cursor is kept with the account and never shown. Secrets are not
//! stored at all until the key vault exists (ADR 0024).

use fs_interfaces::provider::Unsupported;
use fs_objects::{Envelope, ObjectId, Query, StoredObject};
use serde::{Deserialize, Serialize};

use super::consts::{ACCOUNT, CONTENT_TYPE, IDX_ADDRESS, IDX_KIND, SENSITIVE};
use super::handle::Inner;
use super::keys::{account_scope, normalised_address};
use super::preserve::keep_unknown;
use crate::error::{Error, Result};
use crate::model::{Account, AccountSetup};
use crate::provider::ProviderCursor;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct AccountContent {
    pub(super) address: String,
    pub(super) kind: String,
    pub(super) sync_cursor: Option<ProviderCursor>,
}

impl AccountContent {
    /// The identity alias keys hang on; it outlives the account's object id.
    pub(super) fn scope(&self) -> String {
        account_scope(&self.kind, &self.address)
    }
}

impl Inner {
    fn account_view(&self, id: ObjectId, c: AccountContent) -> Account {
        let encryption_ready = self
            .provider(id)
            .is_some_and(|p| p.capabilities().is_supported("encryption.plan"));
        Account {
            id,
            address: c.address,
            kind: c.kind,
            encryption_ready,
        }
    }

    pub(super) async fn load_account(
        &self,
        id: ObjectId,
    ) -> Result<(StoredObject, AccountContent)> {
        let o = self
            .store
            .get(id)
            .await?
            .filter(|o| o.envelope.object_type == ACCOUNT)
            .ok_or(Error::NotFound(id))?;
        let c = serde_json::from_value(o.content.clone())?;
        Ok((o, c))
    }

    pub(super) async fn list_accounts(&self) -> Result<Vec<Account>> {
        let query = Query::new().of_type(ACCOUNT);
        let objects = self.store.query(&query).await?;
        Ok(objects
            .into_iter()
            .filter_map(|o| {
                let content = serde_json::from_value(o.content).ok()?;
                Some(self.account_view(o.envelope.id, content))
            })
            .collect())
    }

    pub(super) async fn add_account(&self, setup: AccountSetup) -> Result<Account> {
        if !setup.secret.is_empty() {
            let why = "there is no key vault yet (ADR 0024); the core does not store secrets";
            return Err(Unsupported::new("account.secret", why).into());
        }
        if setup.address.trim().is_empty() || setup.kind.trim().is_empty() {
            return Err(Error::Invalid(
                "an account needs an address and a kind".into(),
            ));
        }
        if setup.address.contains('|') || setup.kind.contains('|') {
            return Err(Error::Invalid("an account cannot contain `|`".into()));
        }
        let _writes = self.writes.lock().await;
        let same = Query::new()
            .of_type(ACCOUNT)
            .with_index(IDX_ADDRESS, normalised_address(&setup.address))
            .with_index(IDX_KIND, setup.kind.clone());
        if self.store.count(&same).await? > 0 {
            return Err(Error::Conflict(format!(
                "{} is already here",
                setup.address
            )));
        }
        let mut env = Envelope::new(ACCOUNT, setup.address.clone());
        env.sensitive = vec![SENSITIVE.to_string()];
        let id = env.id;
        let content = AccountContent {
            address: setup.address.clone(),
            kind: setup.kind.clone(),
            sync_cursor: None,
        };
        let object = StoredObject::new(env, CONTENT_TYPE, serde_json::to_value(&content)?)
            .with_index(IDX_ADDRESS, normalised_address(&setup.address))
            .with_index(IDX_KIND, setup.kind);
        self.put(object).await?;
        Ok(self.account_view(id, content))
    }

    pub(super) async fn remove_account(&self, id: ObjectId) -> Result<()> {
        let _writes = self.writes.lock().await;
        self.load_account(id).await?;
        self.store.delete(id, &self.config.actor).await?;
        self.unregister(id);
        Ok(())
    }

    pub(super) async fn save_cursor(&self, id: ObjectId, cursor: ProviderCursor) -> Result<()> {
        let _writes = self.writes.lock().await;
        let (mut object, mut content) = self.load_account(id).await?;
        content.sync_cursor = Some(cursor);
        let raw = object.content;
        object.content = serde_json::to_value(&content)?;
        keep_unknown::<AccountContent>(&mut object.content, Some(&raw))?;
        self.put(object).await
    }
}
