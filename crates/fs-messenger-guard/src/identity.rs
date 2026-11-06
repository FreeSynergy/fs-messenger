//! One spelling of an identifier, for comparing.
//!
//! Pattern: Strategy (pure function): `HTTPS://x/Ai` and `https://x/ai` must not be two participants:
//! one could be invited and the other sign. So everything the guard stores and compares goes through
//! this: no surrounding space, lower case. It can only make two names one, which at worst refuses.

/// The form an identifier is stored and compared in.
pub fn id_key(id: &str) -> String {
    id.trim().to_lowercase()
}
