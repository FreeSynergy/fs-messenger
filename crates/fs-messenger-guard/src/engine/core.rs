//! [`GuardEngine`]: the state machine with a repository behind it.
//!
//! Pattern: Facade over State + Repository. `stopped` is a latch in memory, set before anything else
//! happens in a stop, and cleared only by a `reopen` that was written; `pending` holds it for a stop
//! whose proof has not been judged yet. No lock is held while a provider runs, and `check` never
//! needs a lock or the store to see a stop.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
use std::time::Duration;

use chrono::{DateTime, Utc};
use fs_messenger_core::GuardPort;
use tokio::sync::Mutex;

use super::marker;
use crate::clock::{Clock, SystemClock};
use crate::destination::DestinationLookup;
use crate::device::{DeviceVerifier, Ed25519Verifier};
use crate::enrol::LocalEnrolment;
use crate::error::GuardError;
use crate::identity::id_key;
use crate::log::GuardLogEntry;
use crate::permission::{OwnerOnly, StopPermission};
use crate::repo::GuardRepository;
use crate::sensitivity::SensitivityLookup;
use crate::state::{Limits, StopMark};

/// How the guard runs. The numbers are provisional, see `DESIGN.md`.
#[derive(Clone, Debug)]
pub struct GuardConfig {
    /// Actor IRI of the person who owns the program.
    pub owner: String,
    /// The stop marker: a file beside the store, in a folder only this program can write.
    pub stop_marker: PathBuf,
    /// How long a question can be answered.
    pub question_ttl: Duration,
    /// How long an `allowOnce` waits to be used.
    pub approval_ttl: Duration,
    /// Open questions per participant.
    pub open_per_participant: usize,
    /// New questions per participant and hour.
    pub questions_per_hour: u32,
    /// No new question for this long after a person refused one.
    pub refusal_pause: Duration,
    /// Shortest time between two retries of one call.
    pub retry_after: Duration,
    /// Retries of one question before it refuses until a person has answered.
    pub max_retries: u32,
    /// How long all providers together have to end their turns.
    pub turn_wait: Duration,
    /// How long one step waits for the guard's store; no answer reads as unreadable.
    pub store_wait: Duration,
    /// How long the judging of a stop's proof (verifier, permission) has; no answer makes the stop
    /// one by nobody.
    pub judge_wait: Duration,
    /// How long a message on its way in waits for it; no answer reads as not known.
    pub sender_wait: Duration,
    /// How often a stop that could not be written is tried again.
    pub stop_retry: Duration,
    /// At most one log entry for the use of a release, per participant and tool, in this time.
    pub class_log_every: Duration,
}

impl GuardConfig {
    /// The defaults for this owner.
    pub fn new(owner: impl Into<String>, stop_marker: impl Into<PathBuf>) -> Self {
        Self {
            owner: owner.into(),
            stop_marker: stop_marker.into(),
            question_ttl: Duration::from_secs(600),
            approval_ttl: Duration::from_secs(120),
            open_per_participant: 16,
            questions_per_hour: 32,
            refusal_pause: Duration::from_secs(600),
            retry_after: Duration::from_secs(2),
            max_retries: 60,
            turn_wait: Duration::from_secs(5),
            store_wait: Duration::from_secs(2),
            judge_wait: Duration::from_secs(2),
            sender_wait: Duration::from_millis(250),
            stop_retry: Duration::from_secs(2),
            class_log_every: Duration::from_secs(60),
        }
    }
}

/// How often and when a question was asked again. In memory: a restart starts it afresh.
#[derive(Clone, Copy, Debug)]
pub(super) struct Retries {
    pub(super) count: u32,
    pub(super) last: DateTime<Utc>,
}

/// The guard. Tool layers get it as `dyn Gate`, a person's device as `dyn Guard`.
pub struct GuardEngine {
    pub(super) me: Weak<Self>,
    pub(crate) config: GuardConfig,
    pub(crate) repo: Arc<dyn GuardRepository>,
    pub(crate) verifier: Arc<dyn DeviceVerifier>,
    pub(super) permission: Arc<dyn StopPermission>,
    pub(super) clock: Arc<dyn Clock>,
    pub(crate) ops: Mutex<()>,
    pub(super) stopped: AtomicBool,
    /// Counts stops. A `reopen` clears the latch only if no stop came in since it was asked for.
    pub(super) stop_gen: AtomicU64,
    /// Stops whose proof is being judged. They count as stopped.
    pub(super) pending: AtomicUsize,
    pub(super) unsaved: StdMutex<Option<StopMark>>,
    pub(super) retrying: AtomicBool,
    pub(super) retries: StdMutex<HashMap<String, Retries>>,
    pub(super) class_logged: StdMutex<HashMap<(String, String), DateTime<Utc>>>,
    pub(super) port: OnceLock<GuardPort>,
    pub(super) sensitivity: OnceLock<Arc<dyn SensitivityLookup>>,
    pub(super) destinations: OnceLock<Arc<dyn DestinationLookup>>,
}

impl GuardEngine {
    /// Open with Ed25519, only the owner for the stop, and the system clock. The enrolment path comes
    /// back with it and only from here: keep it with the local console, and hand it to nothing else.
    pub fn open(
        config: GuardConfig,
        repo: Arc<dyn GuardRepository>,
    ) -> Result<(Arc<Self>, LocalEnrolment), GuardError> {
        Self::open_with(
            config,
            repo,
            Arc::new(Ed25519Verifier),
            Arc::new(OwnerOnly),
            Arc::new(SystemClock),
        )
    }

    /// Open with other strategies. A repository takes one engine. A stop marker from before reads as
    /// a stop, whatever the store says.
    pub fn open_with(
        mut config: GuardConfig,
        repo: Arc<dyn GuardRepository>,
        verifier: Arc<dyn DeviceVerifier>,
        permission: Arc<dyn StopPermission>,
        clock: Arc<dyn Clock>,
    ) -> Result<(Arc<Self>, LocalEnrolment), GuardError> {
        repo.claim_engine().map_err(GuardError::NotPersisted)?;
        config.owner = id_key(&config.owner);
        let marked = marker::read(&config.stop_marker);
        let engine = Arc::new_cyclic(|me| Self {
            me: me.clone(),
            config,
            repo,
            verifier,
            permission,
            clock,
            ops: Mutex::new(()),
            stopped: AtomicBool::new(marked.is_some()),
            stop_gen: AtomicU64::new(0),
            pending: AtomicUsize::new(0),
            unsaved: StdMutex::new(marked),
            retrying: AtomicBool::new(false),
            retries: StdMutex::new(HashMap::new()),
            class_logged: StdMutex::new(HashMap::new()),
            port: OnceLock::new(),
            sensitivity: OnceLock::new(),
            destinations: OnceLock::new(),
        });
        engine.ensure_retry();
        let enrolment = LocalEnrolment::new(engine.clone());
        Ok((engine, enrolment))
    }

    /// Where marks of objects come from. Once; before it is set every object is `Unknown`, which counts
    /// as marked. Answers `false` if a source was set already.
    pub fn set_sensitivity(&self, source: Arc<dyn SensitivityLookup>) -> bool {
        self.sensitivity.set(source).is_ok()
    }

    /// Where the destinations of questions come from. Once; before it is set every destination is
    /// unknown. Answers `false` if a source was set already.
    pub fn set_destinations(&self, source: Arc<dyn DestinationLookup>) -> bool {
        self.destinations.set(source).is_ok()
    }

    /// The log, oldest first.
    pub async fn log(&self) -> Result<Vec<GuardLogEntry>, String> {
        tokio::time::timeout(self.config.store_wait, self.repo.entries())
            .await
            .unwrap_or_else(|_| Err("the store did not answer".into()))
    }

    /// Whether a stop is waiting to be written to the store.
    pub fn stop_unwritten(&self) -> bool {
        self.unsaved_mark().is_some()
    }

    /// How many questions the retry limit keeps track of.
    pub fn tracked_retries(&self) -> usize {
        self.retries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    pub(super) fn limits(&self) -> Limits {
        let delta = |d: Duration| chrono::Duration::from_std(d).unwrap_or(chrono::TimeDelta::MAX);
        Limits {
            question_ttl: delta(self.config.question_ttl),
            approval_ttl: delta(self.config.approval_ttl),
            open_per_participant: self.config.open_per_participant,
            refusal_pause: delta(self.config.refusal_pause),
            questions_per_hour: self.config.questions_per_hour,
        }
    }

    pub(super) fn is_latched(&self) -> bool {
        self.stopped.load(Ordering::SeqCst) || self.pending.load(Ordering::SeqCst) > 0
    }
}
