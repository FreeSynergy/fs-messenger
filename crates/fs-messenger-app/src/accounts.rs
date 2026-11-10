//! The Backend of the data type `Account`.
//!
//! Pattern: Adapter + Command (`run` picks the call of the core by command name). The secret of an account is never an
//! input (no key vault, ADR 0024).

use std::sync::Arc;

use fs_admin_frontend::{Backend, BackendError, Outcome, Record, Values};
use fs_messenger_core::{AccountSetup, ReceiptChoice};

use crate::convert;
use crate::shared::{Shared, iri, need, need_id, one, put, yes_no};

const TYPE: &str = "Account";

/// Accounts, over the core.
pub struct AccountBackend {
    shared: Arc<Shared>,
}

impl AccountBackend {
    /// A Backend over `shared`.
    pub fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

fn choice(c: ReceiptChoice) -> &'static str {
    match c {
        ReceiptChoice::Shown => "shown",
        ReceiptChoice::Hidden => "hidden",
    }
}

fn parse_choice(text: &str) -> ReceiptChoice {
    if text == "shown" {
        ReceiptChoice::Shown
    } else {
        ReceiptChoice::Hidden
    }
}

impl Backend for AccountBackend {
    /// What the capability table says: the core carries out `command` and this Backend runs it.
    fn supported(&self, command: &str) -> bool {
        self.shared.gate(TYPE, command).is_ok()
    }

    fn list(&self, _query: &Values) -> Result<Vec<Record>, BackendError> {
        let m = &self.shared.messenger;
        Ok(self
            .shared
            .block(m.accounts())?
            .iter()
            .map(convert::account)
            .collect())
    }

    fn run(&self, name: &str, input: &Values) -> Result<Outcome, BackendError> {
        self.shared.gate(TYPE, name)?;
        let m = &self.shared.messenger;
        let mut out = Outcome::default();
        match name {
            "create" => {
                let setup = AccountSetup {
                    address: need(input, "address")?.to_owned(),
                    kind: need(input, "kind")?.to_owned(),
                    secret: String::new(),
                };
                out.id = Some(iri(self.shared.block(m.add_account(setup))?.id));
            }
            "delete" => {
                let id = need_id(input, "account")?;
                self.shared.block(m.remove_account(id))?;
            }
            "receiptChoice" => {
                put(
                    &mut out.values,
                    "choice",
                    choice(self.shared.block(m.receipt_choice())?),
                );
            }
            "setReceiptChoice" => {
                self.shared
                    .block(m.set_receipt_choice(parse_choice(need(input, "choice")?)))?;
            }
            "showReceipt" => {
                let theirs = parse_choice(one(input, "theirs").unwrap_or("hidden"));
                put(
                    &mut out.values,
                    "shown",
                    yes_no(self.shared.block(m.show_receipt(theirs))?),
                );
            }
            _ => return Err(BackendError::NotFound),
        }
        Ok(out)
    }
}
