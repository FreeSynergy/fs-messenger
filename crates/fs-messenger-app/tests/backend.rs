//! The Backends over a real core (in-memory store through the core's test support).
//!
//! Pattern: none (tests against the `Backend` trait).

use std::sync::Arc;

use fs_admin_frontend::{Backend, BackendError, Values};
use fs_messenger_app::{AccountBackend, ConversationBackend, DraftBackend, MessageBackend, Shared};
use fs_messenger_core::{
    Encryption, ExternalId, Ingest, Kind, Messenger, MessengerConfig, MessengerRepo, RawMessage,
};
use fs_objects::{ObjectId, Store, TursoStore};

struct World {
    shared: Arc<Shared>,
    repo: MessengerRepo,
    _runtime: tokio::runtime::Runtime,
}

fn world() -> World {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let repo = runtime.block_on(async {
        let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
        MessengerRepo::open(
            store,
            MessengerConfig::new("https://local.invalid/users/owner"),
        )
        .await
        .unwrap()
    });
    let shared = Shared::new(Arc::new(repo.clone()), runtime.handle().clone());
    World {
        shared,
        repo,
        _runtime: runtime,
    }
}

fn v(pairs: &[(&str, &[&str])]) -> Values {
    pairs
        .iter()
        .map(|(k, vals)| {
            (
                (*k).to_owned(),
                vals.iter().map(|s| (*s).to_owned()).collect(),
            )
        })
        .collect()
}

fn ingest(w: &World, account: ObjectId, id: &str, text: &str) {
    let mut m = RawMessage::new(
        ExternalId::new("uid", id),
        ExternalId::new("thread", "t1"),
        Kind::Mail,
        "ada@example.org",
        text,
    );
    m.encryption = Encryption::Openpgp;
    m.subject = Some("Hello".into());
    w.shared.block(w.repo.ingest(account, m)).unwrap();
}

#[test]
fn an_account_is_added_listed_and_removed() {
    let w = world();
    let b = AccountBackend::new(w.shared.clone());
    assert!(b.list(&Values::new()).unwrap().is_empty());
    let made = b
        .run(
            "create",
            &v(&[("address", &["me@example.org"]), ("kind", &["mail"])]),
        )
        .unwrap();
    let id = made.id.unwrap();
    let list = b.list(&Values::new()).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].values["address"], ["me@example.org"]);
    b.run("delete", &v(&[("account", &[&id])])).unwrap();
    assert!(b.list(&Values::new()).unwrap().is_empty());
}

#[test]
fn the_receipt_choice_is_read_and_set() {
    let w = world();
    let b = AccountBackend::new(w.shared.clone());
    assert_eq!(
        b.run("receiptChoice", &Values::new()).unwrap().values["choice"],
        ["hidden"]
    );
    b.run("setReceiptChoice", &v(&[("choice", &["shown"])]))
        .unwrap();
    assert_eq!(
        b.run("receiptChoice", &Values::new()).unwrap().values["choice"],
        ["shown"]
    );
    let both = b.run("showReceipt", &v(&[("theirs", &["shown"])])).unwrap();
    assert_eq!(both.values["shown"], ["yes"]);
}

#[test]
fn a_command_the_table_marks_unsupported_is_refused_by_name_and_an_unknown_one_is_not_found() {
    let w = world();
    let b = MessageBackend::new(w.shared.clone());
    assert_eq!(
        b.run("react", &Values::new()).unwrap_err(),
        BackendError::Unavailable("unsupported:react".into())
    );
    assert!(!b.supported("react"));
    assert!(b.supported("create"));
    assert_eq!(
        b.run("no-such-command", &Values::new()).unwrap_err(),
        BackendError::NotFound
    );
}

#[test]
fn messages_and_conversations_list_and_a_command_finds_its_conversation() {
    let w = world();
    let account = w
        .shared
        .block(w.repo.add_account(fs_messenger_core::AccountSetup {
            address: "me@example.org".into(),
            kind: "mail".into(),
            secret: String::new(),
        }))
        .unwrap();
    ingest(&w, account.id, "a", "first text");
    let conversations = ConversationBackend::new(w.shared.clone());
    let list = conversations.list(&Values::new()).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].values["kind"], ["mail"]);
    assert_eq!(list[0].values["unreadCount"], ["1"]);

    let messages = MessageBackend::new(w.shared.clone());
    let all = messages.list(&Values::new()).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].values["content"], ["first text"]);
    assert_eq!(all[0].values["encryption"], ["openpgp"]);
    assert_eq!(all[0].values["unread"], ["yes"]);
    let id = all[0].id.clone();

    let in_conversation = messages
        .list(&v(&[("conversation", &[&list[0].id])]))
        .unwrap();
    assert_eq!(in_conversation.len(), 1);
    assert!(
        messages
            .list(&v(&[("text", &["nothing like it"])]))
            .unwrap()
            .is_empty()
    );
    assert_eq!(messages.list(&v(&[("text", &["first"])])).unwrap().len(), 1);

    messages
        .run("read", &v(&[("message", &[&id]), ("unread", &["no"])]))
        .unwrap();
    assert_eq!(
        messages.list(&Values::new()).unwrap()[0].values["unread"],
        ["no"]
    );
    messages
        .run("flag", &v(&[("message", &[&id]), ("flagged", &["yes"])]))
        .unwrap();
    assert_eq!(
        messages.list(&Values::new()).unwrap()[0].values["flagged"],
        ["yes"]
    );

    conversations
        .run(
            "pin",
            &v(&[("conversation", &[&list[0].id]), ("pinned", &["yes"])]),
        )
        .unwrap();
    assert_eq!(
        conversations.list(&Values::new()).unwrap()[0].values["pinned"],
        ["yes"]
    );
    let export = conversations
        .run("export", &v(&[("conversation", &[&list[0].id])]))
        .unwrap();
    assert!(export.values["export"][0].contains("first text"));

    messages.run("delete", &v(&[("message", &[&id])])).unwrap();
    assert!(messages.list(&Values::new()).unwrap().is_empty());
}

#[test]
fn a_message_that_was_not_listed_is_not_found() {
    let w = world();
    let b = MessageBackend::new(w.shared.clone());
    let id = format!("urn:uuid:{}", ObjectId::new());
    assert_eq!(
        b.run("flag", &v(&[("message", &[&id]), ("flagged", &["yes"])]))
            .unwrap_err(),
        BackendError::NotFound
    );
}

#[test]
fn a_draft_is_saved_changed_listed_and_discarded() {
    let w = world();
    let b = DraftBackend::new(w.shared.clone());
    let made = b
        .run(
            "create",
            &v(&[("to", &["bob@example.org"]), ("text", &["first"])]),
        )
        .unwrap();
    let id = made.id.unwrap();
    let list = b.list(&Values::new()).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].values["text"], ["first"]);
    b.run("change", &v(&[("draft", &[&id]), ("text", &["second"])]))
        .unwrap();
    let list = b.list(&Values::new()).unwrap();
    assert_eq!(list[0].values["text"], ["second"]);
    assert_eq!(
        list[0].values["to"],
        ["bob@example.org"],
        "what the change does not name stays"
    );
    b.run("delete", &v(&[("draft", &[&id])])).unwrap();
    assert!(b.list(&Values::new()).unwrap().is_empty());
}

#[test]
fn sending_without_a_provider_is_refused_and_says_so() {
    let w = world();
    let b = MessageBackend::new(w.shared.clone());
    let err = b
        .run(
            "create",
            &v(&[("to", &["bob@example.org"]), ("text", &["hello"])]),
        )
        .unwrap_err();
    assert!(matches!(err, BackendError::Unavailable(_)), "{err:?}");
}
