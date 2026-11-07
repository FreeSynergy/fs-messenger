//! Doubles for `DestinationLookup`: one that says what it is told and keeps what it was asked, one that
//! never answers, one that panics.
#![allow(dead_code)]
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_messenger_guard::{Destination, DestinationLookup};

/// Says what it is told to, and keeps what it was asked.
pub struct Says {
    pub answer: Mutex<Destination>,
    pub asked: Mutex<Vec<(String, String)>>,
}

impl Says {
    pub fn new(answer: Destination) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(answer),
            asked: Mutex::new(vec![]),
        })
    }
    pub fn now_says(&self, answer: Destination) {
        *self.answer.lock().unwrap() = answer;
    }
}

#[async_trait]
impl DestinationLookup for Says {
    async fn destination(&self, participant: &str, program: &str) -> Destination {
        self.asked
            .lock()
            .unwrap()
            .push((participant.into(), program.into()));
        self.answer.lock().unwrap().clone()
    }
}

pub struct Hangs;
#[async_trait]
impl DestinationLookup for Hangs {
    async fn destination(&self, _: &str, _: &str) -> Destination {
        std::future::pending().await
    }
}

pub struct Panics;
#[async_trait]
impl DestinationLookup for Panics {
    async fn destination(&self, _: &str, _: &str) -> Destination {
        panic!("the source broke");
    }
}

pub fn known(words: &str) -> Destination {
    Destination::Known(words.into())
}
