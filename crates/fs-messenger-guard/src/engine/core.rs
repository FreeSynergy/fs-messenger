//! [`GuardEngine`]: the state machine with a repository behind it.
//!
//! Pattern: Facade over State + Repository. Every operation reads the record through, so a record that
//! went bad is seen at once and reads as stopped. `stopped` is a latch in memory, set before anything
//! else happens in a stop and cleared only by a `reopen` that was written; a stop that could not be
//! written stays in `unsaved`, is laid over every read, and goes out with the next write that works.
//! No lock is held while a provider runs, and `check` never needs `ops` to see a stop.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use fs_messenger_core::GuardPort;
use tokio::sync::Mutex;

use crate::clock::{Clock, SystemClock};
use crate::device::{DeviceVerifier, Ed25519Verifier};
use crate::digest::unhex;
use crate::enrol::LocalEnrolment;
use crate::log::GuardLogEntry;
use crate::permission::{OwnerOnly, StopPermission};
use crate::repo::GuardRepository;
use crate::state::{GuardDoc, Limits, StopMark};

/// How the guard runs. The numbers are provisional, see `DESIGN.md`.
#[derive(Clone, Debug)]
pub struct GuardConfig {
    /// Actor IRI of the person who owns the program.
    pub owner: String,
    /// How long a question can be answered.
    pub question_ttl: Duration,
    /// How long an `allowOnce` waits to be used.
    pub approval_ttl: Duration,
    /// Open questions per participant.
    pub open_per_participant: usize,
    /// Shortest time between two retries of one call.
    pub retry_after: Duration,
    /// Retries of one question before it refuses until a person has answered.
    pub max_retries: u32,
    /// How long all providers together have to end their turns.
    pub turn_wait: Duration,
}

impl GuardConfig {
    /// The defaults for this owner.
    pub fn new(owner: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            question_ttl: Duration::from_secs(600),
            approval_ttl: Duration::from_secs(120),
            open_per_participant: 16,
            retry_after: Duration::from_secs(2),
            max_retries: 60,
            turn_wait: Duration::from_secs(5),
        }
    }
}

/// How often and when a question was asked again. In memory: a restart starts it afresh.
#[derive(Clone, Copy, Debug)]
pub(super) struct Retries {
    pub(super) count: u32,
    pub(super) last: DateTime<Utc>,
}

/// What a read of the state found.
#[derive(Debug)]
pub(crate) enum View {
    /// There is none: a fresh program.
    Missing,
    /// The store did not answer.
    Unreadable(String),
    /// It answered with something that is not a state.
    Corrupt(String),
    /// A state.
    Doc(Box<GuardDoc>),
}

/// The guard. Tool layers get it as `dyn Gate`, a person's device as `dyn Guard`.
pub struct GuardEngine {
    pub(crate) config: GuardConfig,
    pub(crate) repo: Arc<dyn GuardRepository>,
    pub(crate) verifier: Arc<dyn DeviceVerifier>,
    pub(super) permission: Arc<dyn StopPermission>,
    pub(super) clock: Arc<dyn Clock>,
    pub(crate) ops: Mutex<()>,
    pub(super) stopped: AtomicBool,
    pub(super) unsaved: StdMutex<Option<StopMark>>,
    pub(super) retries: StdMutex<HashMap<String, Retries>>,
    pub(super) port: OnceLock<GuardPort>,
}

impl GuardEngine {
    /// Open with Ed25519, only the owner for the stop, and the system clock. The enrolment path comes
    /// back with it and only from here: keep it with the local console, and hand it to nothing else.
    pub fn open(
        config: GuardConfig,
        repo: Arc<dyn GuardRepository>,
    ) -> (Arc<Self>, LocalEnrolment) {
        Self::open_with(
            config,
            repo,
            Arc::new(Ed25519Verifier),
            Arc::new(OwnerOnly),
            Arc::new(SystemClock),
        )
    }

    /// Open with other strategies.
    pub fn open_with(
        config: GuardConfig,
        repo: Arc<dyn GuardRepository>,
        verifier: Arc<dyn DeviceVerifier>,
        permission: Arc<dyn StopPermission>,
        clock: Arc<dyn Clock>,
    ) -> (Arc<Self>, LocalEnrolment) {
        let engine = Arc::new(Self {
            config,
            repo,
            verifier,
            permission,
            clock,
            ops: Mutex::new(()),
            stopped: AtomicBool::new(false),
            unsaved: StdMutex::new(None),
            retries: StdMutex::new(HashMap::new()),
            port: OnceLock::new(),
        });
        let enrolment = LocalEnrolment::new(engine.clone());
        (engine, enrolment)
    }

    /// What a device has to sign for: the engine's identity. `None` while there is no readable state.
    pub async fn engine_identity(&self) -> Option<Vec<u8>> {
        match self.view().await {
            View::Doc(doc) => unhex(&doc.engine),
            _ => None,
        }
    }

    /// What is wrong with the state, if it cannot be read. Such a state reads as stopped.
    pub async fn damage(&self) -> Option<String> {
        match self.view().await {
            View::Unreadable(why) | View::Corrupt(why) => Some(why),
            View::Missing | View::Doc(_) => None,
        }
    }

    /// The log, oldest first.
    pub async fn log(&self) -> Result<Vec<GuardLogEntry>, String> {
        self.repo.entries().await
    }

    pub(super) fn limits(&self) -> Limits {
        let delta = |d: Duration| chrono::Duration::from_std(d).unwrap_or(chrono::TimeDelta::MAX);
        Limits {
            question_ttl: delta(self.config.question_ttl),
            approval_ttl: delta(self.config.approval_ttl),
            open_per_participant: self.config.open_per_participant,
        }
    }

    pub(super) fn is_latched(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    pub(super) fn mark_unsaved(&self, mark: StopMark) {
        self.stopped.store(true, Ordering::SeqCst);
        *self.unsaved.lock().unwrap_or_else(PoisonError::into_inner) = Some(mark);
    }

    pub(crate) fn clear_stop(&self) {
        *self.unsaved.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.stopped.store(false, Ordering::SeqCst);
    }

    fn unsaved_mark(&self) -> Option<StopMark> {
        self.unsaved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The state as it stands, with a stop that could not be written laid over it. Needs no lock.
    pub(crate) async fn view(&self) -> View {
        match self.repo.load().await {
            Err(why) => View::Unreadable(why),
            Ok(None) => View::Missing,
            Ok(Some(value)) => match GuardDoc::from_value(value) {
                Err(why) => View::Corrupt(why),
                Ok(mut doc) => {
                    if let Some(mark) = self.unsaved_mark() {
                        doc.apply_stop(&mark);
                    }
                    View::Doc(Box::new(doc))
                }
            },
        }
    }

    /// Write. A stop laid over this state is written with it, and is then no longer unsaved.
    pub(crate) async fn save(&self, doc: &GuardDoc) -> Result<(), String> {
        let value = doc.to_value()?;
        self.repo.save(&value).await?;
        if self.unsaved_mark().is_some() && doc.mode == crate::model::GuardMode::Stopped {
            *self.unsaved.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        Ok(())
    }

    /// Try again to write a stop that could not be written. Never in the way of anything.
    pub(super) async fn flush_stop(&self) {
        if self.unsaved_mark().is_none() {
            return;
        }
        let Ok(_ops) = self.ops.try_lock() else {
            return;
        };
        if let View::Doc(doc) = self.view().await {
            // A failure stays as it was: reported when the stop was pulled, tried again next time.
            let _ = self.save(&doc).await;
        }
    }
}
