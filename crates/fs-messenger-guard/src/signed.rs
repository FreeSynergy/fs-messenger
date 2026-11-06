//! What a device signs. The encoding is fixed here and in `DESIGN.md`.
//!
//! Pattern: Builder (pure function): one way to get from a request to bytes. Every field is written as
//! a 4-byte big-endian length and then its bytes, and the arguments have their count in front, so a
//! byte cannot move from one field to the next, and `("ab", "c")` differs from `("a", "bc")`. The
//! layout is: context, engine identity, method, signer, count, arguments ..., nonce.

/// The exact context string. A signature for anything else does not fit.
pub const CONTEXT: &str = "FreeSynergy.Messenger guard device proof v1";

/// The method a proof is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `Guard.invite`.
    Invite,
    /// `Guard.release`.
    Release,
    /// `Guard.revoke`.
    Revoke,
    /// `Guard.stop`.
    Stop,
    /// `Guard.reopen`.
    Reopen,
    /// `MessageAction.decide`.
    Decide,
    /// Local enrolment: add a device key.
    AddDevice,
    /// Local enrolment: take a device key away.
    RemoveDevice,
}

impl Method {
    /// Its name in the signed bytes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Invite => "invite",
            Self::Release => "release",
            Self::Revoke => "revoke",
            Self::Stop => "stop",
            Self::Reopen => "reopen",
            Self::Decide => "decide",
            Self::AddDevice => "addDevice",
            Self::RemoveDevice => "removeDevice",
        }
    }
}

fn field(out: &mut Vec<u8>, bytes: &[u8]) -> Option<()> {
    out.extend_from_slice(&u32::try_from(bytes.len()).ok()?.to_be_bytes());
    out.extend_from_slice(bytes);
    Some(())
}

/// The bytes a device signs. `None` if a field does not fit its length; that is refused.
pub fn signed_bytes(
    engine: &[u8],
    method: Method,
    signer: &str,
    arguments: &[&[u8]],
    nonce: &[u8],
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    field(&mut out, CONTEXT.as_bytes())?;
    field(&mut out, engine)?;
    field(&mut out, method.as_str().as_bytes())?;
    field(&mut out, signer.as_bytes())?;
    out.extend_from_slice(&u32::try_from(arguments.len()).ok()?.to_be_bytes());
    for argument in arguments {
        field(&mut out, argument)?;
    }
    field(&mut out, nonce)?;
    Some(out)
}

/// The arguments of `decide`, in their order: message id, tool call id, option id, digest, and the
/// revision as 8 big-endian bytes. A decision carries no nonce: it has an empty one, and is single use
/// through these arguments, which name one revision of one call.
pub fn decide_arguments<'a>(
    message: &'a str,
    tool_call: &'a str,
    option: &'a str,
    digest: &'a [u8],
    revision: &'a [u8; 8],
) -> [&'a [u8]; 5] {
    [
        message.as_bytes(),
        tool_call.as_bytes(),
        option.as_bytes(),
        digest,
        revision,
    ]
}
