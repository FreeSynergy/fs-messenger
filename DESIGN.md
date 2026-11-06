# fs-messenger – Design

`fs-messenger` is one program for mail, chat and agents (ADR 0018, 0054). This repo holds the **program core**
(`crates/fs-messenger-core`: no GUI, no RPC, no real connectors). A message is an AS2 **`Note`** envelope + content + immutable
original (ADR 0020, 0030), stored through `fs-store`. Frontends never see protocols; connectors plug in as `Provider`s (ADR 0045).

## State

Version 0.1.0, not tagged. Mirrors `messenger.capnp` draft 0.2 of `fs-interfaces` v0.16.0 (consumers: 0 of 2). Generated types are not in the public API.

| Area | State |
|---|---|
| Model (serde): `Message`, `MessageSummary`, `ConversationSummary`, `Account`, `Kind`, `Encryption`, `DeliveryState`, `SenderKind`, `ToolCall` with all contract fields, `Attachment`, `Reaction`, `Usage`, events | done |
| Fail-safe values: unset or unrecognised `Encryption`, `risk`, `ToolCallKind`, `ToolCallState`, `SenderKind`, `Kind`, `DeliveryState` = `unknown`; `choice` = `refuse`; unrecognised `outcome` = `unknown`, unset `open`, which the contract reads as "not known". `MessageAction` has no default, `None` is refused. Holds for old and newer records, tested through the program | done |
| Unreadable records: a page skips them and lists them in `skipped`; they cannot block a list, the next page, `next_unread`, `mark_read` or the newest message | done |
| Store form: `Note` envelope, `context` = conversation id, `inReplyTo`, vendor `freesynergy.net:encryption`, index fields, `original` | done |
| Dedupe: an alias the provider issued, also kept by a tombstone. A `Message-ID` only links | done, provisional (see decisions) |
| `Messenger` over `MessengerRepo`: accounts, conversation list / open / next unread, messages page, get, mark read up to, pin, move, act (delete, flag, unread, forward), search, drafts, attachment bytes, usage | done |
| Cursor paging: keyset on fixed-width sort keys, so new messages do not shift a page | done |
| Read mark, next unread and unread count over a stored local arrival sequence; display by the sender's date | done, see "Reading" |
| Change feed: `subscribe` maps store activities to `created`, `updated`, `deleted`, `state`, `toolCall`; a lagging listener catches up from the log; dropping the `Subscription` ends it | done |
| `Provider` (Strategy): `kind`, `capabilities`, `fetch`, `send`, `encryption_plan`, all with an `Unsupported` default. Registered per account at runtime; one send uses one provider, the one that planned it | done, real connectors come in their own crates |
| Ingest as Template method (rules, lock, dedupe, store) and `sync`: the cursor moves after a stored batch, a refused message counts as `rejected` and does not stop it, `more` without progress ends it | done |
| Encrypted only, outgoing: `Draft.send` and `Forward` run the plan; refusal is `Error::EncryptionRefused` naming recipients; no override. A reported downgrade ends the draft, so a retry cannot send again | done |
| Encrypted only, incoming: stored and marked with what the provider says; `none`, `transport-only` and `unknown` are never end-to-end; no quarantine | done |
| Encrypted only, at rest: see "Encryption at rest" | done |
| Tool calls as stored data: store, update in place, read; engine id and revision; `settled` final; an outside question is `working` | done |
| What a connector found: `RawMessage.undecrypted` and `signature` (`SignatureCheck`, unset is `notVerified`), kept on the `Message` as typed fields, not in the text, and not in the contract yet. The fingerprint of a good signature stays with the provider | done |
| Mail threading: `RawMessage.references`, sender-chosen, newest 64 kept, only link. `Outgoing::references` gives the parent's chain, and a sent message continues it | done |
| Size limits at ingest: 25 MiB a message, named refusal `TooLarge`; search reads 256 KiB of a body | done, provisional |
| Tests | 120 against real `TursoStore` (in memory, or temp files where encryption matters); the `Provider` doubles only feed core logic |

### Local only: says so here, and claims nothing more

Flag, read and unread state, pin, folder and delete are this device's own state: no provider is told, and the other side does not learn of them. Delete here means "delete for me". **Messages are device-local for now.** "On all devices" from `requirements.md` is **not met**, and this is the strategy's decision (ADR 0029 point 2, hub store unencrypted, key vault of ADR 0024 missing): every message carries a sensitive category and `fs-hub` withholds marked objects, so messages do not sync between devices through the hub.

### Not built: answers `Unsupported`, or says so here

| What | How it shows |
|---|---|
| Edit and react | `Unsupported("message.edit" / "message.react")`, nothing changes: `Provider` has no method to carry them, and a local success would be a claim that nothing reached the other side. Answering them needs a provider method first. Taking a reaction back cannot log AS2 `Undo` with `fs-store` as it is: `Undo` is state-changing, and it refuses that as a log entry |
| AI guard: release, brake, emergency stop. `MessageAction::Decide` | `Unsupported("toolcall.decide")`; the guard follows, handled by the engine, never by a provider |
| `Conversation.stop` | `Unsupported("turn.stop")` |
| Live text (`Event::Text`, `TextChunk`) | the type exists, nothing emits it; `text.stream` reported unsupported |
| Rules (Sieve) | `Unsupported("rule.read" / "rule.write")` |
| Scheduled send | a future `at` is `Unsupported("message.schedule")`; the draft is saved, nothing left |
| Account secrets | a non-empty secret is `Unsupported("account.secret")`, because there is no key vault (ADR 0024); the core stores none |
| `bodyHtml` | always empty: there is no sanitiser and the field promises sanitised HTML; the raw form stays in `original` |
| `senderKind` from the sender's card | not built: always `unknown`, and `RawMessage` has no such field, so no remote can claim `person` |
| Reactions of others | no path in |
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
| `ingest_rules.rs` | Specification | what a delivered message must be, as pure rules |
| `repo/{accounts,conversations,messages,attachments}.rs` | Repository | one concern each |
| `repo/actions.rs`, `repo/deletion.rs`, `repo/conversation_ops.rs`, `model/action.rs` | Command | `MessageAction` and its executor; small edits |
| `repo/send.rs`, `repo/send_record.rs`, `repo/send_receipt.rs` | Template method + State | check first; `sending` -> `sent` or `failed` |
| `repo/forward.rs`, `repo/api.rs` | Facade | forward is a send; wiring only |
| `repo/search.rs`, `repo/capabilities.rs` | Builder | filter to query; capability report |
| `repo/{keys,text}.rs` | Strategy (pure functions) | one canonical key and name rule |
| `repo/paging.rs`, `model/page.rs` | Iterator | cursor pages |
| `model/*` | Value objects | contract names, fail-safe defaults |

## Store layout (provisional)

Types: `Note`, `Tombstone`, `Document` (attachment), `Collection` (conversation), `freesynergy.net:Draft`, `freesynergy.net:Account`, `freesynergy.net:StoreCheck`. Content type `application/activity+json`; `content` = body text, `mediaType` = its type.

`fs-store` has no tables of ours, so the alias table of `data/messages/technical.md` is three index fields, `alias` (provider-issued), `link` (sender-chosen) and `receipt` (from sending), each `<kind>:<normalised address>|<scheme>|<external id>`. Other index fields: `arrival`, `arrival_desc`, `desc`, `unread`, `flagged`, `attachment`, `blob`, `from`, `text`, `encryption`, `kind`, `job`, `task`, `account`; conversations: `sort`, `unread`, `folder`, `conv_alias`.

## Encryption at rest (ADR 0018, 0024, 0029)

`Store` **cannot be asked** whether it is encrypted: `is_encrypted` exists only on the concrete `TursoStore`. So:

* Every object this program writes carries the sensitive category, so `fs-store`'s own refusal holds for each of them (tested for all types).
* `MessengerRepo::open_turso(Arc<TursoStore>)` is the **only constructor of a normal build**. It asks `is_encrypted`, so it refuses every plain store, also one opened with `allow_sensitive_unencrypted`, and an in-memory one.
* It then tries to write a sensitive check object, the same check any other backend would need. Later opens offer that object a different original, which an encrypted store rejects as `OriginalImmutable` after its encryption check, so the log does not grow with every start.
* `MessengerRepo::open(dyn Store)` exists only behind the feature `test-support`, for tests over a plain in-memory store. Connector crates may turn it on in their dev-dependencies. No normal build can name it.

**Limit:** a non-Turso backend cannot use this program until the `Store` trait has `is_encrypted`, which is wanted from `fs-store`.

**One handle per store.** Clone a `MessengerRepo`; do not open a second one on the same store. Two would share no write lock, so a dedupe could interleave, and no arrival counter.

## Reading

**Display** goes by the sender's date, newest first; on equal dates the last arrived is on top. A date in the future shows as written but sorts at its arrival, so it cannot hold the top of the list.

**"Read up to X", `next_unread` and `unread_count`** go by a local arrival sequence stored with each message, never by a date a sender chose (`mark_read` sets the flag that `unread_count` counts). Read up to X marks what arrived at or before X in this conversation. A later arrival is never marked, whatever its date:

* A mail dated 1970 that arrives after the mark stays unread, though it is shown far down, and `next_unread` finds it.
* A mail dated 2999 cannot make what arrived after it read.
* **When a frontend passes the top message:** everything that had arrived by the time that message arrived reads as read. A message shown below it that arrived **later** stays unread, even if the person has seen it on screen: it has an older date and came after the top one. That is the rule, chosen so a sender's date cannot decide what counts as read. It shows when dates and arrival disagree (delayed mail, a re-sync). A frontend that wants "all I see" must pass the last arrived message on screen; `Message.arrival` tells which, and the page's summaries do not carry it yet (open point).

## Decisions

* **Aliases have an authority (provisional, by the owner's review).** Only what a provider issued (UID, JMAP email id, Matrix event id) recognises a replay: `RawMessage.aliases`. What a sender chose (`Message-ID`) only links replies: `RawMessage.links`; a `message-id` in `aliases` is moved there, and a message with nothing else is refused. One exception: what a receipt names matches a returning copy of what we sent, and only if the copy's sender is the account's own address. **Consequence: the same mail in two IMAP folders shows twice unless the provider says otherwise. A duplicate beats a lost mail.**
* **Aliases hang on the account's identity,** kind plus normalised address, so removing and adding an account does not make its mail new, and a reply goes out over the account as it is now. Addresses compare without case or surrounding space. Provisional: the same mail in two accounts is two messages, and a sender in one account cannot hide a message in another.
* **Enforced in the core, not in providers.** The plan comes from the provider, the verdict from `gate.rs`. No provider, a provider without a plan, `none`, `transport-only`, `unknown`, or a good plan with a missing key: all refused, hidden recipients counted. A plan naming nobody blames all recipients.
* **The provider is not trusted further than it can be.** The core cannot read ciphertext. It can only refuse beforehand and report afterwards: a receipt weaker than end-to-end stores the message as reported and answers `EncryptionDowngraded`. A provider that lies, or sends on its own, is out of reach of this crate: connectors must be reviewed.
* **Agent input claims nothing.** `id`, `revision`, `risk`, `options`, `argumentsDigest`, `askedOf` are never taken from a report, `detail` moves into `summary` (contract: agent text goes there only). An agent's number is never compared to the engine's revision. An empty reference is refused, because a replay would add the call again.
* **An outside question is `working`.** It is shown and not asked: nobody here can answer it, and `refused` would claim something the agent never hears, while `settled` being final would hide what it does next. The contract wording is being corrected accordingly.
* **What a stranger claims about a reply goes, the mail stays.** A self-link is dropped, a link that would close a cycle is given up, a `|` in a sender-chosen id drops that id. A `|` in an alias or a conversation alias is refused by name, so connectors must not build those from sender text unencoded.
* **An outgoing claim needs the account's own address as sender;** otherwise it is an ordinary incoming mail.
* **Provisional: limits.** 25 MiB for a message, all parts counted, `Error::TooLarge`; configurable. Search reads the first 256 KiB of a body and stops there; the message itself stays whole. NUL is taken out of text; the original keeps every byte.
* **A body type other than plain or markdown falls back to `text/plain`.**
* **`Kind`, `DeliveryState`, `ToolCallState`, `ToolCallOutcome` have `unknown` here and not in the contract,** whose ordinal 0 of each is a real value. See the open points: the server must decide how to send them.
* **Delete = delete + tombstone.** The original cannot be removed by an update (immutable), so the object is deleted and an AS2 `Tombstone` under the same id keeps aliases and order.
* **A message gets its own id at sending, not the draft's**, because the store keeps `published` of an existing object and a message must sort by when it was sent. Its id stays through edits.
* **Provisional: message order.** Newest first by `published`, which the sender writes and can forge within the limits above. Page size 50, configurable: the contract has none.
* **Provisional: attachments** are store objects holding the bytes as their original; `Attachment.blob` is that id, to be wrapped in the contract's `Blob` capability by the server. They go when nothing names them. Forward shares them.
* **`pd:Communication`** is the sensitive category of everything, verified as existing in DPV 2.3. It stays for now. `fs-hub` withholds marked objects, see "Local only".

## Open points

Contract, for the owner:

1. `Compose.draft(replyTo)` has no account: with several accounts nothing says which one sends. The core adds `Draft::set_account` (provisional, refused on a reply); with exactly one account, it is taken.
2. `Draft.setSubject` is on branch `msg-contract-2` of `fs-interfaces`, not tagged. `Draft::set_subject` mirrors the name with no dependency change; `Outgoing::subject` exposes it.
3. No page size, and no direction for `messages`.
4. `Kind`, `DeliveryState`, `ToolCallState`, `ToolCallOutcome` have no `unknown` on the wire. The server can't send what the core holds without a guess, so these need the same ordinal-0 treatment as `Encryption`, or a decision.
5. `Message` holds `undecrypted` and `signature`, which the contract has no field for.
6. `MessageSummary` has no arrival, so a frontend cannot tell which message on screen arrived last (see "Reading").

Code:

1. **Send is not exactly-once.** A store failure right after a provider accepted answers an error and keeps the draft open, so a retry can send twice. A failed send stays visible, and its retry is a second message.
2. **Echo race.** A copy fetched while a send is under way, before the receipt is stored, is not recognised and is stored as a second message. Needs a provider-side client id.
3. **Delete takes two store steps;** a crash in between lets a replay revive it. The store's log keeps the old image, content included, until compaction; the program must call `compact()`, which only removes entries acknowledged by the hub. Since messages do not sync through the hub, nothing acknowledges them yet.
4. **Mark read** writes one transaction per message. An unreadable record stays counted as unread until it is repaired.
5. **Performance.** Pages load whole objects, originals included; search is a substring scan (`fs-store` has no tokenised search), and body text is held twice for it, up to its limit.
6. Removing an account keeps its messages, and those conversations can no longer send until the account comes back.
7. A claimed parent that never arrives stays pending.
8. The lag catch-up re-reads the log from the last seen id; on a read failure it waits for the next lag.
9. A forged `Message-ID` can still misthread a reply, as links are the sender's word. Harmless to delivery by design.

## Crates

Feature `test-support`: `MessengerRepo::open`, off in every normal build.

`fs-store` (git tag v0.2.0), `fs-interfaces` (git tag v0.16.0): `Capabilities`, `Unsupported`. `serde`, `serde_json`, `chrono`, `thiserror`, `async-trait`, `tokio`; dev: `tempfile`. No network crates in this crate. `scripts/test-servers` belongs to the connector work, not to this crate.
