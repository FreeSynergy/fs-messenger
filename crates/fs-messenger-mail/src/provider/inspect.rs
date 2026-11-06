//! [`Inspection`]: what the signature and the encryption of a stored original say.
//!
//! Pattern: Facade over the reading flow. The core has no field for a signature or for "encrypted, but
//! not readable here" (`DESIGN.md`), so a frontend asks this with `Message.original`. It derives again
//! from the original (ADR 0030), with the keys held now.

use fs_messenger_core::Encryption;

use super::facade::MailProvider;
use crate::error::MailError;
use crate::openpgp::SignatureState;

/// What a mail really is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    /// `openpgp` only if it was decrypted, just now, with a key held here.
    pub encryption: Encryption,
    /// It claims to be encrypted and could not be opened.
    pub undecrypted: bool,
    /// What the signature says.
    pub signature: SignatureState,
}

impl MailProvider {
    /// Look at the bytes of a stored original.
    pub async fn inspect(&self, original: &[u8]) -> Result<Inspection, MailError> {
        let read = self.read_mail(original.to_vec()).await?;
        Ok(Inspection {
            encryption: read.encryption,
            undecrypted: read.undecrypted,
            signature: read.signature,
        })
    }
}
