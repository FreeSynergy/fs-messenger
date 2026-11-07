//! The first source of destinations: it knows none.
//!
//! Pattern: Null object: implements [`DestinationLookup`] and learns nothing, on purpose. It answers
//! `Unknown` for every participant and program, so every question says "unknown" where the text goes.
//! The real source replaces it through `GuardEngine::set_destinations`.

use async_trait::async_trait;

use crate::destination::{Destination, DestinationLookup};

/// Knows no destinations.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDestinations;

#[async_trait]
impl DestinationLookup for NoDestinations {
    async fn destination(&self, _participant: &str, _program: &str) -> Destination {
        Destination::Unknown
    }
}
