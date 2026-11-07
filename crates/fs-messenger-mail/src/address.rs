//! One rule for comparing mail addresses.
//!
//! Pattern: Strategy (a pure function), so keys, plans and Autocrypt agree on who is who.

/// The form under which an address is looked up: trimmed, lower case, without `<...>`.
///
/// Provisional: the local part is case-sensitive in theory (RFC 5321); in practice nobody relies on
/// it, and Autocrypt 1.1 compares the same way.
pub fn normalise_address(address: &str) -> String {
    let inner = match (address.rfind('<'), address.rfind('>')) {
        (Some(a), Some(b)) if a < b => &address[a + 1..b],
        _ => address,
    };
    inner.trim().to_lowercase()
}

/// Whether this can be an address at all. A control character in one could end up in a command or a
/// header, and it never names a mailbox.
pub(crate) fn clean_address(address: &str) -> bool {
    !address.contains(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_case_do_not_matter() {
        assert_eq!(
            normalise_address("Bob <BOB@Example.org>"),
            "bob@example.org"
        );
        assert_eq!(normalise_address(" bob@example.org "), "bob@example.org");
    }
}
