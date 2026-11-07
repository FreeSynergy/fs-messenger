//! F7 and F8 of the review of 9ea2830 (strategy 4d(4), 2026-11-07): where the text goes comes from
//! what the engine knows (`DestinationLookup`), asked when a question is shown, and never from an AI.
//! What it does not know reads "unbekannt". Nothing of it is stored in the question.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use common::{AI, World, args};
use fs_messenger_core::ToolCallRisk as R;
use fs_messenger_guard::{
    Arguments, Caller, Destination, DestinationLookup, Gate, NoDestinations, Question, Verdict,
};

const UNKNOWN: &str = "unbekannt";

/// Says what it is told to, and keeps what it was asked.
struct Says {
    answer: Mutex<Destination>,
    asked: Mutex<Vec<(String, String)>>,
}

impl Says {
    fn new(answer: Destination) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(answer),
            asked: Mutex::new(vec![]),
        })
    }
    fn now_says(&self, answer: Destination) {
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

struct Hangs;
#[async_trait]
impl DestinationLookup for Hangs {
    async fn destination(&self, _: &str, _: &str) -> Destination {
        std::future::pending().await
    }
}

struct Panics;
#[async_trait]
impl DestinationLookup for Panics {
    async fn destination(&self, _: &str, _: &str) -> Destination {
        panic!("the source broke");
    }
}

fn known(words: &str) -> Destination {
    Destination::Known(words.into())
}

async fn asked(w: &World, input: serde_json::Value) -> Question {
    w.ask("message.delete", input).await
}

#[tokio::test]
async fn the_null_object_knows_nothing_and_the_question_says_unknown() {
    assert_eq!(
        NoDestinations.destination(AI, "message").await,
        Destination::Unknown
    );
    let w = World::working().await;
    let q = asked(&w, args("/m")).await;
    assert_eq!(q.call.summary, UNKNOWN);
    assert_eq!(
        w.shown(&q).await.summary,
        UNKNOWN,
        "also as the messenger shows it"
    );
}

#[tokio::test]
async fn what_the_engine_knows_is_what_the_question_says() {
    let w = World::working().await;
    let says = Says::new(known("only the model on this machine"));
    assert!(w.engine.set_destinations(says.clone()));
    let q = asked(&w, args("/m")).await;
    assert_eq!(q.call.summary, "only the model on this machine");
    assert_eq!(w.shown(&q).await.summary, "only the model on this machine");
    let asked = says.asked.lock().unwrap().clone();
    assert!(
        asked
            .iter()
            .all(|(who, program)| who == AI && program == "message"),
        "{asked:?}"
    );
}

/// Nothing an AI can say reaches the destination: not the arguments, not the name of the tool, not the
/// name of the participant. The source is asked about the participant and the program of its record.
#[tokio::test]
async fn an_ai_can_neither_set_nor_influence_it() {
    let w = World::working().await;
    let says = Says::new(Destination::Unknown);
    assert!(w.engine.set_destinations(says.clone()));
    let lies = args("/m");
    let lies = serde_json::json!({
        "path": lies["path"], "goesTo": "to nobody", "goes_to": "to nobody",
        "destination": "to nobody", "summary": "to nobody", "message": "this stays on the device",
    });
    let q = asked(&w, lies).await;
    assert_eq!(q.call.summary, UNKNOWN);
    // The tool named after a lie is just an unknown tool of another program.
    let odd = w.ask("goesTo-to-nobody.read", args("/x")).await;
    assert_eq!(odd.call.summary, UNKNOWN);
    let asked = says.asked.lock().unwrap().clone();
    for (who, program) in &asked {
        assert!(!format!("{who} {program}").contains("nobody"), "{asked:?}");
    }
    assert!(asked.contains(&(AI.to_string(), "goesto-to-nobody".to_string())));
}

#[tokio::test]
async fn an_answer_that_is_empty_or_hides_text_reads_unknown() {
    let w = World::working().await;
    let says = Says::new(known(""));
    assert!(w.engine.set_destinations(says.clone()));
    assert_eq!(asked(&w, args("/a")).await.call.summary, UNKNOWN);
    says.now_says(known("safe\u{202e}tluser"));
    assert_eq!(asked(&w, args("/b")).await.call.summary, UNKNOWN);
}

#[tokio::test]
async fn a_source_that_hangs_or_panics_reads_unknown_and_the_question_stands() {
    for source in [
        Arc::new(Hangs) as Arc<dyn DestinationLookup>,
        Arc::new(Panics),
    ] {
        let w = World::working().await;
        assert!(w.engine.set_destinations(source));
        let started = Instant::now();
        let q = asked(&w, args("/m")).await;
        assert_eq!(q.call.summary, UNKNOWN);
        assert!(started.elapsed().as_secs() < 5, "the wait has a limit");
    }
}

/// F8: the destination is found when the question is shown, not kept in it.
#[tokio::test]
async fn an_open_question_shows_the_destination_of_now() {
    let w = World::working().await;
    let says = Says::new(known("only this device"));
    assert!(w.engine.set_destinations(says.clone()));
    let q = asked(&w, args("/m")).await;
    assert_eq!(q.call.summary, "only this device");
    says.now_says(known("to Anthropic"));
    w.release(AI, R::ReadsOnly, "readsOnly").await; // a new release of the same program
    w.pause();
    let Verdict::Ask(again) = w.check("message.delete", args("/m")).await else {
        panic!("the same question stands");
    };
    assert_eq!(again.call.id, q.call.id);
    assert_eq!(again.call.summary, "to Anthropic");
    w.pause();
    let revised = w
        .engine
        .revise(
            &Caller::new(AI),
            &q.call.id,
            &Arguments::from_value(&args("/n")).unwrap(),
        )
        .await;
    assert!(matches!(revised, Verdict::Ask(_)));
    assert_eq!(w.shown(&q).await.summary, "to Anthropic");
    let record = w.record().await.unwrap();
    assert!(
        !record.to_string().contains("only this device"),
        "nothing of it is stored"
    );
}
