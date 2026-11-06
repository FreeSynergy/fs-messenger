//! The guard is asked again before the first byte, not only when the send comes in. The real guard, with
//! one behaviour added around it: from its second answer on, it refuses, as a stop in between would.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use common::wire::{llama, serve, stream};
use common::{AI, World};
use fs_messenger_ai::{ModelProvider, TurnFailure};
use fs_messenger_core::{Error, ToolCallOutcome};
use fs_messenger_guard::{Arguments, Caller, Gate, Refusal, Verdict};

struct RefusesFromTheSecondAsk {
    real: Arc<dyn Gate>,
    asked: AtomicUsize,
}

#[async_trait]
impl Gate for RefusesFromTheSecondAsk {
    async fn check(&self, caller: &Caller, capability: &str, arguments: &Arguments) -> Verdict {
        self.real.check(caller, capability, arguments).await
    }
    async fn revise(&self, caller: &Caller, call: &str, arguments: &Arguments) -> Verdict {
        self.real.revise(caller, call, arguments).await
    }
    async fn finished(
        &self,
        caller: &Caller,
        call: &str,
        outcome: ToolCallOutcome,
    ) -> Result<(), Refusal> {
        self.real.finished(caller, call, outcome).await
    }
    async fn admit_message(&self, sender: &str) -> Result<(), Refusal> {
        if self.asked.fetch_add(1, Ordering::SeqCst) == 0 {
            self.real.admit_message(sender).await
        } else {
            Err(Refusal::Stopped)
        }
    }
}

#[tokio::test]
async fn a_refusal_between_the_send_and_the_first_byte_keeps_everything_here() {
    let world = World::invited().await;
    let server = serve(vec![stream(llama::answer(&["never"]))]).await;
    let mut link = world.link();
    link.gate = Arc::new(RefusesFromTheSecondAsk {
        real: world.engine.clone(),
        asked: AtomicUsize::new(0),
    });
    let provider = ModelProvider::llama_cpp(world.config(&server.origin), link)
        .await
        .unwrap();
    let provider = Arc::new(provider);
    world.past_the_gate(&provider);
    let mut events = provider.live_events();
    let sent = world.person_sends(AI, "hello").await;
    assert!(
        matches!(sent, Err(Error::EncryptionDowngraded { .. })),
        "{sent:?}"
    );
    let done = world.finished(&provider, &mut events).await;
    assert_eq!(done.failure(), TurnFailure::NotAdmitted);
    assert_eq!(server.requests().len(), 0);
    assert!(
        done.stored.is_none(),
        "nothing stored without the guard's word"
    );
}
