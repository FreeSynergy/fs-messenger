# fs-messenger – Design

`fs-messenger` is one program for mail, chat and agents (ADR 0018, 0054). This repo holds the **program core**
(`crates/fs-messenger-core`: no GUI, no RPC, no real connectors). A message is an AS2 **`Note`** envelope + content + immutable
original (ADR 0020, 0030), stored through `fs-store`. Frontends never see protocols; connectors plug in as `Provider`s (ADR 0045).

## State

Version 0.1.0, not tagged. Mirrors `messenger.capnp` draft 0.2 of `fs-interfaces` v0.16.0 (consumers: 0 of 2). Generated types are not in the public API.

| Area | State |
|---|---|
| Model (serde): `Message`, `MessageSummary`, `ConversationSummary`, `Account`, `Kind`, `Encryption`, `DeliveryState`, `SenderKind`, `ToolCall` with all contract fields, `Attachment`, `Reaction`, `Usage`, events | done |
| Fail-safe defaults: unset or unrecognised `Encryption` = `unknown`, `risk` = `unknown`, `choice` = `refuse`, `ToolCallKind` = `unknown`, `SenderKind` = `unknown`; `MessageAction` has no default, `None` is refused. Holds for old records (tested through the program) | done |
| Store form: `Note` envelope, `context` = conversation id, `inReplyTo`, vendor `freesynergy.net:encryption`, index fields, `original` | done |
| Dedupe: aliases `(scheme, external id)`, also kept by a tombstone, so neither a replay nor a re-sync stores twice or revives a deleted message | done, provisional scope (see decisions) |
| `Messenger` over `MessengerRepo`: accounts, conversation list / open / next unread, messages page, get, mark read up to, pin, move, act (delete, flag, unread, react, forward, edit), search, drafts, attachment bytes, usage | done |
| Cursor paging: keyset on fixed-width sort keys, so new messages do not shift a page | done |
| Change feed: `subscribe` maps store activities to `created`, `updated`, `deleted`, `state`, `toolCall`; a lagging listener catches up from the log; dropping the `Subscription` ends it | done |
| `Provider` (Strategy): `kind`, `capabilities`, `fetch`, `send`, `encryption_plan`, all with an `Unsupported` default. Registered per account at runtime | done, real connectors come in their own crates |
| Ingest as Template method (check, lock, dedupe, store) and `sync` (cursor moves after a stored batch) | done |
| Encrypted only, outgoing: `Draft.send` and `Forward` run the plan; refusal is `Error::EncryptionRefused` naming recipients; no override | done |
| Encrypted only, incoming: stored and marked with what the provider says; `none`, `transport-only` and `unknown` are never end-to-end; no quarantine | done |
| Encrypted only, at rest: see "Encryption at rest" | done, with a named limit |
| Tool calls as stored data: store, update in place, read; engine id; `settled` final; lower revision ignored | done |
| Tests | 72, against real `TursoStore` (in memory, or temp files where encryption matters); the `Provider` double only feeds core logic |

### Not built: answers `Unsupported`, or says so here

| What | How it shows |
|---|---|
| AI guard: release, brake, emergency stop. `MessageAction::Decide` | `Unsupported("toolcall.decide")`; the guard follows, handled by the engine, never by a provider |
| `Conversation.stop` | `Unsupported("turn.stop")` |
| Live text (`Event::Text`, `TextChunk`) | the type exists, nothing emits it; `text.stream` reported unsupported |
| Rules (Sieve) | `Unsupported("rule.read" / "rule.write")` |
| Scheduled send | a future `at` is `Unsupported("message.schedule")`; the draft is saved, nothing left |
| Account secrets | a non-empty secret is `Unsupported("account.secret")`, because there is no key vault (ADR 0024); the core stores none |
| `bodyHtml` | always empty: there is no sanitiser and the field promises sanitised HTML; the raw form stays in `original` |
| Carrying edit, delete, react and read to the network | applied locally only; `Provider` has no method for it. A local edit does **not** reach the other side yet |
| Reactions of others | no path in; only my own are stored |
| Key discovery, key verification, devices, groups, presence, notifications, import/export, Cap'n Proto server, MCP, GUI, real connectors, Telegram, own mail server, ratings | not in this step |
| Link to the library guide for unencrypted mail (ADR 0031) | a frontend's job; the core gives the mark |

## Structure and pattern map (ADR 0043)

Public contract: `Messenger`, `Draft`, `Provider`, `Ingest`, `Listener`, small value types. `MessengerRepo` is the implementation; its parts live on the private `Inner`.

| Module | Pattern | Why |
|---|---|---|
| `messenger.rs` | Repository (interface) + Null Object | the program's contract; every method exists, the default says `Unsupported` |
| `draft.rs`, `repo/draft_handle.rs` | State | open until sent or discarded; a refused send leaves it open |
| `provider/traits.rs` | Strategy | one trait for mail, Matrix, MIMI, agents (ADR 0045) |
| `provider/outgoing.rs` | Newtype as proof | `Outgoing` can only be built after the check, so no caller can hand a provider anything else |
| `gate.rs` | Specification | the rule of ADR 0018 in one place, as pure functions |
| `ingest.rs`, `repo/ingest.rs` | Template method | fixed flow, store-backed steps |
| `listener.rs`, `repo/subscription.rs` | Observer + RAII guard | contract's listener; dropping ends it |
| `repo/feed.rs` | Adapter | store activity to contract events |
| `repo/handle.rs` | Facade + Registry | one entry; providers per account |
| `repo/store_check.rs` | Specification over a real write | the store cannot be asked, so its behaviour is tested |
| `repo/{message,conversation}_codec.rs`, `repo/draft_data.rs`, `repo/ingest_store.rs`, `repo/tool_calls.rs` | Adapter | model to envelope + content + index |
| `repo/{accounts,conversations,messages,attachments}.rs` | Repository | one concern each |
| `repo/actions.rs`, `repo/deletion.rs`, `model/action.rs` | Command | `MessageAction` and its executor |
| `repo/send.rs`, `repo/send_record.rs` | Template method + State | check first; `sending` -> `sent` or `failed` |
| `repo/forward.rs`, `repo/api.rs` | Facade | forward is a send; wiring only |
| `repo/search.rs`, `repo/capabilities.rs` | Builder | filter to query; capability report |
| `repo/{keys,text}.rs` | Strategy (pure functions) | one canonical key and name rule |
| `repo/paging.rs`, `model/page.rs` | Iterator | cursor pages |
| `model/*` | Value objects | contract names, fail-safe defaults |

## Store layout (provisional)

Types: `Note`, `Tombstone`, `Document` (attachment), `Collection` (conversation), `freesynergy.net:Draft`, `freesynergy.net:Account`, `freesynergy.net:StoreCheck`. Content type `application/activity+json`; `content` = body text, `mediaType` = its type.

`fs-store` has no tables of ours, so the alias table of `data/messages/technical.md` is the index field `alias` = `<account>|<scheme>|<external id>`. Other index fields: `order`, `desc`, `unread`, `flagged`, `attachment`, `blob`, `from`, `text`, `encryption`, `kind`, `job`, `task`, `account`; conversations: `sort`, `unread`, `folder`, `conv_alias`.

## Encryption at rest (ADR 0018, 0024, 0029)

`Store` **cannot be asked** whether it is encrypted: `is_encrypted` exists only on the concrete `TursoStore`. So:

* Every object this program writes carries the sensitive category, so `fs-store`'s own refusal holds for each of them (tested for all types).
* `MessengerRepo::open(dyn Store)` tries to write a sensitive check object. A plain store answers `EncryptionRequired`, which becomes `Error::StoreNotEncrypted`, and nothing is written (tested with a real plain `TursoStore`). There is no other constructor. Later opens offer the object a different original, which an encrypted store rejects as `OriginalImmutable` after its encryption check, so the log does not grow with every start.
* `MessengerRepo::open_turso(Arc<TursoStore>)` additionally asks `is_encrypted`. **A program binary should use this one.**

**Limit:** a store opened with `allow_sensitive_unencrypted = true` passes `open(dyn Store)`, because it accepts sensitive objects; `open_in_memory` is such a store, and tests use it. Tested and accepted as the store's documented opt-out for "the volume itself is encrypted"; `open_turso` refuses it. Closing this for every backend needs `is_encrypted` on the `Store` trait: wanted from `fs-store`.

## Decisions

* **Enforced in the core, not in providers.** The plan comes from the provider, the verdict from `gate.rs`. No provider, a provider without a plan, `none`, `transport-only`, `unknown`, or a good plan with a missing key: all refused, hidden recipients counted. A plan naming nobody blames all recipients.
* **The provider is not trusted further than it can be.** The core cannot read ciphertext. It can only refuse beforehand and report afterwards: a receipt weaker than end-to-end stores the message as reported and answers `EncryptionDowngraded`. A provider that lies, or sends on its own, is out of reach of this crate: connectors must be reviewed.
* **Agent input claims nothing.** `id`, `revision`, `risk`, `options`, `argumentsDigest`, `askedOf` are never taken from a report, `detail` moves into `summary` (contract: agent text goes there only).
* **Provisional: outside calls keep state `question`.** The contract says an outside call "is shown, but cannot be answered here", and that the engine settles as `refused` what it cannot allow. Every call reaches this version from outside, and no engine decides, so nothing is settled as `refused`: the agent would not learn of it, and `settled` being final would hide what it does next. To settle when the guard exists.
* **Provisional: revision.** The engine's revision is raised on each real change. An offered non-zero revision below the held one is ignored; zero means "no order given", because an agent need not number its reports.
* **Provisional: aliases per account.** A sender in one account cannot hide a message in another by claiming its `Message-ID`. The same mail in two accounts is two messages. Within one account a forged `Message-ID` can still hide a mail: connectors should alias on the provider's own id (IMAP UID, JMAP id) as well.
* **Delete = delete + tombstone.** The original cannot be removed by an update (immutable), so the object is deleted and an AS2 `Tombstone` under the same id keeps aliases and order.
* **A message gets its own id at sending, not the draft's**, because the store keeps `published` of an existing object and a message must sort by when it was sent. Its id stays through edits.
* **Provisional: message order.** Newest first by `published`, which comes from the sender and can be forged; "read up to X" follows the same order. Page size 50, configurable: the contract has none.
* **Provisional: attachments** are store objects holding the bytes as their original; `Attachment.blob` is that id, to be wrapped in the contract's `Blob` capability by the server. They go when nothing names them. Forward shares them.
* **Provisional: `pd:Communication`** as the sensitive category of everything. Taken from memory of DPV-PD and not checked against the vocabulary here. Please check.
* **`senderKind`** takes what the provider says and is `unknown` for own messages. Resolving it from the card in `fs-people` is not built, so a provider could still claim `person`.

## Open points

Contract, for the owner:

1. `Compose.draft(replyTo)` has no account: with several accounts nothing says which one sends. The core adds `Draft::set_account` (provisional); with exactly one account, it is taken.
2. `Draft` has no subject: a new mail's name comes from its text.
3. No page size, and no direction for `messages`.
4. `react` allows one own reaction per message, as `react(key)`, empty = undo, implies; to confirm.

Code:

1. **Send is not exactly-once.** A store failure right after a provider accepted answers an error and keeps the draft, so a retry can send twice. A failed send stays visible, and its retry is a second message.
2. **Echo race.** An echo fetched while a send is under way, before the receipt is stored, is not recognised and is stored as a second message. Needs a provider-side client id.
3. **Delete takes two store steps;** a crash in between lets a replay revive it. The store's log keeps the old image, content included, until compaction; the program must call `compact()`, which only removes entries acknowledged by the hub.
4. **Mark read** writes one transaction per message.
5. **Performance.** Pages load whole objects, originals included; search is a substring scan (`fs-store` has no tokenised search), and the full text is held twice for it.
6. Removing an account keeps its messages, and those conversations can no longer send.
7. A repeated `in_reply_to` to an unknown parent stays pending until that parent arrives, which may be never.
8. The lag catch-up re-reads the log from the last seen id; on a read failure it waits for the next lag.

## Crates

`fs-store` (git tag v0.2.0), `fs-interfaces` (git tag v0.16.0): `Capabilities`, `Unsupported`. `serde`, `serde_json`, `chrono`, `thiserror`, `async-trait`, `tokio`; dev: `tempfile`. No network crates in this crate. `scripts/test-servers` belongs to the connector work, not to this crate.
