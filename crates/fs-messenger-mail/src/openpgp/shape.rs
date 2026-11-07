//! What an armored OpenPGP message is made of, read from its packet headers: [`Shape`], [`method_of`].
//!
//! Pattern: Specification (a pure reader). It names the encryption packets and their versions and
//! nothing else: no key is needed and nothing is decrypted. A frontend that wants to say "older method"
//! about a stored original asks [`method_of`]; the tests ask [`Shape`] whether a message mixes
//! the two methods (RFC 9580 sections 5.1.1 and 5.1.2), which it must never do.

use std::io::{BufReader, Cursor, Read};

use pgp::armor::Dearmor;

use super::Method;

/// The encryption packets of a message, in order, with their versions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shape {
    /// Versions of the PKESK packets (tag 1).
    pub pkesk: Vec<u8>,
    /// The data packet that follows: tag and version. `(18, 1)` is a v1 SEIPD, `(18, 2)` a v2 SEIPD,
    /// `(9, 0)` an SED without integrity, `(20, 1)` `GnuPG`'s AEAD packet. `None` if there is none.
    pub data: Option<(u8, u8)>,
}

impl Shape {
    /// The method the message was written with, if its packets are one of the two legal pairs: v3
    /// PKESK before a v1 SEIPD, or v6 PKESK before a v2 SEIPD. Anything else is `None`.
    pub fn method(&self) -> Option<Method> {
        let (pkesk, data) = (&self.pkesk, self.data?);
        let all = |v: u8| !pkesk.is_empty() && pkesk.iter().all(|p| *p == v);
        match data {
            (18, 1) if all(3) => Some(Method::Older),
            (18, 2) if all(6) => Some(Method::Modern),
            _ => None,
        }
    }

    /// Read the packet headers of `armored`. Bytes that are not an armored message give an empty shape.
    pub fn of(armored: &[u8]) -> Self {
        let mut binary = Vec::new();
        let mut dearmor = Dearmor::new(BufReader::new(Cursor::new(armored)));
        if dearmor.read_to_end(&mut binary).is_err() {
            return Self::default();
        }
        Self::of_binary(&binary)
    }

    fn of_binary(mut rest: &[u8]) -> Self {
        let mut shape = Self::default();
        while let Some((tag, header, length)) = header(rest) {
            let body = rest.get(header..).unwrap_or_default();
            let first = body.first().copied().unwrap_or(0);
            match tag {
                1 => shape.pkesk.push(first),
                9 | 18 | 20 => {
                    shape.data = Some((tag, if tag == 9 { 0 } else { first }));
                    return shape;
                }
                _ => {}
            }
            match length {
                Some(n) if body.len() >= n => rest = &body[n..],
                _ => return shape,
            }
        }
        shape
    }
}

/// Tag, length of the header, and length of the body if it is known (not partial or open).
fn header(b: &[u8]) -> Option<(u8, usize, Option<usize>)> {
    let ctb = *b.first()?;
    if ctb & 0x80 == 0 {
        return None;
    }
    if ctb & 0x40 == 0 {
        let (tag, size) = ((ctb >> 2) & 0x0f, ctb & 3);
        let width = [1, 2, 4, 0][usize::from(size)];
        let n = b
            .get(1..=width)?
            .iter()
            .fold(0usize, |a, x| (a << 8) | usize::from(*x));
        return Some((tag, 1 + width, (width > 0).then_some(n)));
    }
    let tag = ctb & 0x3f;
    let first = usize::from(*b.get(1)?);
    match first {
        0..=191 => Some((tag, 2, Some(first))),
        192..=223 => {
            let second = usize::from(*b.get(2)?);
            Some((tag, 3, Some(((first - 192) << 8) + second + 192)))
        }
        255 => {
            let n = b
                .get(2..6)?
                .iter()
                .fold(0usize, |a, x| (a << 8) | usize::from(*x));
            Some((tag, 6, Some(n)))
        }
        _ => Some((tag, 2, None)),
    }
}

/// The method of an armored message, if its packets make one of the two legal pairs.
pub fn method_of(armored: &[u8]) -> Option<Method> {
    Shape::of(armored).method()
}
