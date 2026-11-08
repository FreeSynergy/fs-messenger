# fs-messenger – Design

`fs-messenger` is one program for mail, chat and agents (ADR 0018, 0054). This repo holds the **program core**
(`crates/fs-messenger-core`: no GUI, no RPC, no real connectors) and the **AI guard** (`crates/fs-messenger-guard`, its own `DESIGN.md`). A message is an AS2 **`Note`** envelope + content + immutable
original (ADR 0020, 0030), stored through `fs-objects`. Frontends never see protocols; connectors plug in as `Provider`s (ADR 0045).

## State

Latest tag v0.4.0 (README/DESIGN header corrected 2026-11-08; earlier: 0.2.0 was 0.1.1: `EngineGuard` and `SenderLookup` got new methods for the guard crate). Mirrors the `messenger.capnp` contract, version 0.3 (`CONTRACT_VERSION`), with the dependency on `fs-interfaces` v0.27.0 (consumers: 0 of 2). Generated types are not in the public API.

| Area | State |
|---|---|
| Model (serde): `Message`, `MessageSummary`, `ConversationSummary`, `Account`, `Kind`, `Encryption`, `DeliveryState`, `SenderKind`, `ToolCall` with all contract fields, `Attachment`, `Reaction`, `Usage`, events | done |
| Fail-safe values: unset or unrecognised `Encryption`, `risk`, `ToolCallKind`, `ToolCallState`, `SenderKind`, `Kind`, `DeliveryState` = `unknown`; `choice` = `refuse`; `signature` = `notVerified`; unrecognised `outcome` = `unknown`, unset `open`, which the contract reads as "not known". `MessageAction` has no default, `None` is refused. Holds for old and newer records, and rewriting a record keeps the value it could not read, unless this version really changed it | done |
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
| Hooks of the guard: `EngineGuard`, `SenderLookup`, `install_guard` (once) and its `GuardPort`, `Provider::stop_turns`, `ToolCallDecision.revision`, `Error::DecisionRefused`, `Error::GuardRefused`, `DecisionProblem`, index `open_call`, announcements beside the store feed. Accounts are transports only (`mail`, `matrix`, `mimi`); there is no account kind `agent`: an AI is a participant on a normal account, and `SenderLookup` says who | done, see `crates/fs-messenger-guard/DESIGN.md` |
| What a connector found: `RawMessage.undecrypted` and `signature` (`SignatureCheck`, unset is `notVerified`, and always `notVerified` without a sender or readable content), kept on `MessageSummary` as typed fields, not in the text, mirroring the contract. The fingerprint of a good signature stays with the provider | done |
| Mail threading: `RawMessage.references`, sender-chosen, newest 64 kept, only link. `Outgoing::references` gives the parent's chain, and a sent message continues it | done |
| Size limits at ingest and in `record_tool_call`: see "Limits". Over the limit a stub stays (`Ingested::TooLarge`, `MessageSummary.too_large`, 0 = it is not), so the mail is visible, not gone (ADR 0030): aliases, links, sender, date, subject, size, no content. A connector that only holds a cut-off start says so with `RawMessage.too_large`: the core stubs it whatever the limit and never stores those bytes as the original | done, provisional |
| Large bodies: over 64 KiB in a store object of their own, see "What a large body costs" | done, with a limit named there |
| An undecrypted message: no ciphertext in preview, name or search; it cannot be forwarded; delivered again under a provider-issued id in readable form it is replaced under the same id, keeping what the person did | done |
| Tests | 167 against real `TursoStore` (in memory, or temp files where encryption matters); the `Provider` doubles only feed core logic |
| Guard tests | see `crates/fs-messenger-guard/DESIGN.md` |

### Local only: says so here, and claims nothing more

Flag, read and unread state, pin, folder and delete are this device's own state: no provider is told, and the other side does not learn of them. Delete here means "delete for me". **Messages are device-local for now.** "On all devices" from `requirements.md` is **not met**, and this is the strategy's decision (ADR 0029 point 2, hub store unencrypted, key vault of ADR 0024 missing): every message carries a sensitive category and `fs-hub` withholds marked objects, so messages do not sync between devices through the hub.

### Not built: answers `Unsupported`, or says so here

| What | How it shows |
|---|---|
| Edit and react | `Unsupported("message.edit" / "message.react")`, nothing changes: `Provider` has no method to carry them, and a local success would be a claim that nothing reached the other side. Answering them needs a provider method first. Taking a reaction back cannot log AS2 `Undo` with `fs-objects` as it is: `Undo` is state-changing, and it refuses that as a log entry |
| `MessageAction::Decide` and `Conversation.stop` without an installed guard | `Unsupported("toolcall.decide")`, `Unsupported("turn.stop")`, which is the safe default. With `fs-messenger-guard` installed they go there |
| Live text (`Event::Text`, `TextChunk`) | the type exists, nothing emits it; `text.stream` reported unsupported |
| Rules (Sieve) | `Unsupported("rule.read" / "rule.write")` |
| Scheduled send | a future `at` is `Unsupported("message.schedule")`; the draft is saved, nothing left |
| Account secrets | a non-empty secret is `Unsupported("account.secret")`, because there is no key vault (ADR 0024); the core stores none |
| `bodyHtml` | always empty: there is no sanitiser and the field promises sanitised HTML; the raw form stays in `original` |
| `senderKind` from the sender's card | `SenderLookup` is the hook; the guard answers `ai` for the invited. Without it always `unknown`, and `RawMessage` has no such field, so no remote can claim `person` |
| Reactions of others | no path in |
| Key discovery, key verification, devices, groups, presence, notifications, import/export, Cap'n Proto server, MCP, GUI, real connectors, Telegram, own mail server, ratings | not in this step |
| Link to the library guide for unencrypted mail (ADR 0031) | a frontend's job; the core gives the mark |
| AIs in conversations (ADR 0054, section "KIs in Unterhaltungen") | **not built**: a notice before an AI joins; all members must agree, else it stays out; a later member accepts it or stays out; consent withdrawn means the AI must leave; no history from before it joined; no exception for local AIs. Nothing here checks it yet |

## Structure and pattern map (ADR 0043)

Public contract: `Messenger`, `Draft`, `Provider`, `Ingest`, `Listener`, small value types. `MessengerRepo` is the implementation; its parts live on the private `Inner`.

| Module | Pattern | Why |
|---|---|---|
| `messenger.rs` | Repository (interface) + Null Object | the program's contract; every method exists, the default says `Unsupported` |
| `draft.rs`, `repo/draft_handle.rs` | State | open until sent or discarded; a refused send leaves it open |
| `provider/traits.rs` | Strategy | one trait for mail, Matrix, MIMI, agents (ADR 0045) |
| `guard_hook.rs` | Strategy | traits `EngineGuard` (incl. `open_for_reports`) and `SenderLookup`: the core knows these, not the guard crate |
| `repo/guard_port.rs`, `repo/guard_turns.rs` | Facade | the one port back into the messenger, handed out once; `stop_turns` is a method of it with no trait of provided methods, so no Template method |
| `repo/guard_note.rs` | Adapter | an engine call becomes a local `Note` |
| `provider/turn.rs` | none | a plain data type; guard `DESIGN.md` open point 0 |
| `provider/outgoing.rs` | Newtype as proof | `Outgoing` can only be built after the check, so no caller can hand a provider anything else |
| `gate.rs` | Specification | the rule of ADR 0018 in one place, as pure functions |
| `ingest.rs`, `repo/ingest.rs` | Template method | fixed flow, store-backed steps |
| `listener.rs`, `repo/subscription.rs` | Observer + RAII guard | contract's listener; dropping ends it |
| `repo/feed.rs` | Adapter | store activity to contract events |
| `repo/handle.rs` | Facade + Registry | one entry; providers per account |
| `repo/store_check.rs` | Specification over a real write | the store cannot be asked, so its behaviour is tested |
| `repo/{message,conversation}_codec.rs`, `repo/message_index.rs`, `repo/draft_data.rs`, `repo/ingest_store.rs` | Adapter | model to envelope + content + index |
| `repo/tool_calls.rs` | Adapter, Strategy | an agent's report mapped onto `ToolCall`; while the guard (`EngineGuard::open_for_reports`) says shut, a report from an `ai` or `unknown` sender is kept `cancelled` |
| `repo/preserve.rs` | Memento | keeps what this version could not read through a rewrite |
| `repo/ingest_upgrade.rs` | State | undecrypted to readable, one way |
| `ingest_rules.rs` | Specification | what a delivered message must be, as pure rules |
| `repo/{accounts,conversations,messages,attachments,body,ingest_threads}.rs` | Repository | one concern each |
| `repo/actions.rs`, `repo/deletion.rs`, `repo/conversation_ops.rs`, `model/action.rs` | Command | `MessageAction` and its executor; small edits |
| `repo/send.rs`, `repo/send_record.rs`, `repo/send_receipt.rs` | Template method + State | check first; `sending` -> `sent` or `failed` |
| `repo/forward.rs`, `repo/api.rs` | Facade | forward is a send; wiring only |
| `repo/search.rs`, `repo/capabilities.rs` | Builder | filter to query; capability report |
| `repo/{keys,text}.rs` | Strategy (pure functions) | one canonical key and name rule |
| `repo/paging.rs`, `model/page.rs` | Iterator | cursor pages |
| `model/*` | Value objects | contract names, fail-safe defaults |

## Store layout (provisional)

Types: `Note`, `Tombstone`, `Document` (attachment), `Collection` (conversation), `freesynergy.net:Draft`, `freesynergy.net:Account`, `freesynergy.net:StoreCheck`. Content type `application/activity+json`; `content` = body text, `mediaType` = its type.

`fs-objects` has no tables of ours, so the alias table of `data/messages/technical.md` is three index fields, `alias` (provider-issued), `link` (sender-chosen) and `receipt` (from sending), each `<kind>:<normalised address>|<scheme>|<external id>`. Other index fields: `arrival`, `arrival_desc`, `desc`, `unread`, `flagged`, `open_call`, `attachment`, `blob`, `from`, `text`, `encryption`, `kind`, `job`, `task`, `account`; conversations: `sort`, `unread`, `folder`, `conv_alias`.

## Encryption at rest (ADR 0018, 0024, 0029)

`Store` **cannot be asked** whether it is encrypted: `is_encrypted` exists only on the concrete `TursoStore`. So:

* Every object this program writes carries the sensitive category, so `fs-objects`'s own refusal holds for each of them (tested for all types).
* `MessengerRepo::open_turso(Arc<TursoStore>)` is the **only constructor unless a build asks for `test-support`**. It asks `is_encrypted`, so it refuses every plain store, also one opened with `allow_sensitive_unencrypted`, and an in-memory one.
* It then tries to write a sensitive check object, the same check any other backend would need. Later opens offer that object a different original, which an encrypted store rejects as `OriginalImmutable` after its encryption check, so the log does not grow with every start.
* `MessengerRepo::open(dyn Store)` exists only behind the feature `test-support`, for tests over a plain in-memory store. Connector crates may turn it on in their dev-dependencies. The feature is off unless a build asks for it, so it is a guard against mistakes, not against someone who turns it on. `tests/compile_fail.rs` builds a real crate without it and checks that `open` does not exist.

**Limit:** a non-Turso backend cannot use this program until the `Store` trait has `is_encrypted`, which is wanted from `fs-objects`.

**One handle per store.** Clone a `MessengerRepo`; do not open a second one on the same store. Two would share no write lock, so a dedupe could interleave, and no arrival counter. A second open of the same `Arc` in one process is refused with `Conflict`, until the last clone of the first is gone. A second `TursoStore` opened on the same file is another object and is **not** detected: `fs-objects` gives no identity to ask for, so this part stays a rule, documented here.

## Reading

**Display** goes by the sender's date, newest first; on equal dates the last arrived is on top. A date in the future shows as written but sorts at its arrival, so it cannot hold the top of the list.

**"Read up to X"** sweeps a message only if it is **both** at or before X in local arrival, a sequence stored with each message and never from a sender, **and** at or below X on screen. `next_unread` walks the arrival order. `unread_count` counts the result.

* A mail dated 1970 that arrives after the mark stays unread: it is below X, but arrived later.
* Reading a backdated mail does not take along the newer, unseen mail above it: that arrived earlier, but is above X.
* A mail dated 2999 cannot make what arrived after it read.
* **When a frontend passes the top message:** everything below it on screen that had arrived by then reads as read. A message shown below it that arrived **later** stays unread, even if the person has seen it on screen. So a person who has read everything visible can still have unread mail after one "read up to the top", when dates and arrival disagree (delayed mail, a re-sync). That case, "read all I see", **stays open**. `MessageSummary.arrival` lets a frontend see the difference, and it can mark such a message on its own.

## Limits (provisional)

The limit, 25 MiB by default, applies to **metadata + the larger of (the original, what was decoded)**. Metadata is subject, conversation name, sender, recipients, job and task references, links, references, aliases, tool calls with their results, attachment names and types. Decoded is body, reasoning and attachment bytes. So a mail's decoded parts are not counted on top of the original they came from, and they cannot hide behind a small original either.

At most 256 attachments: more is treated as too large, whatever they weigh. **Metadata other than tool calls over 64 KiB is treated as too large too**, since it stays in the message: every change logs it again, and a list page carries the name. Tool calls are outside that cap, because what a tool returned can rightly be larger; they count towards the 25 MiB. A conversation name has at most 200 characters. At most 64 links (the first) and 64 references (the newest). An id may have 2048 bytes. In `record_tool_call`, the limit applies to the tool calls of a message plus its inline body, and a report over it is refused with `Error::TooLarge` and changes nothing.

Search reads the first 256 KiB of a body and stops there; the message itself stays whole. NUL is taken out of text, and every control character out of sender, recipients, subject and names, because they go on into replies and mail headers; `set_subject` strips them and `set_recipients` refuses them. The original keeps every byte.

## What a large body costs

Each change of a message logs a before and an after image of it, content and index included, and those stay until compaction. A body over 64 KiB therefore lives in a store object of its own, as the immutable original of a `Document`. `fs-objects` leaves originals out of update images, so a flag does not copy it. Measured on a 5 MiB body: four flag changes add 2.0 MiB of log (2,110,236 bytes), where the same body inside `content` adds 42 MiB (44,053,060 bytes).

**What this does not solve, and what `fs-objects` would have to do:**

1. **Creating still doubles a body, and more.** A Create image carries the original, base64: the 5 MiB body measured 6.9 MiB of log (7,257,991 bytes) on top of its 5 MiB in the object. Scaled up, not measured, a 25 MiB body is then about 60 MiB, somewhat **more** than the 51 MiB reported for inline text. Deleting it adds another image. The same holds for every attachment and every mail original. Needed: `fs-objects` keeps large originals in one place and logs a reference, for instance content-addressed.
2. **The index is copied by every change.** Update images include index fields, so up to 256 KiB of search text is copied twice per change: that is the 2.0 MiB above. Needed: images without the index, which can be derived. Not worked around here: a search index in another object would need a second query for every filter.
3. Reasoning and tool results stay in `content`, within the limits. Drafts keep their whole text in `content`, and are only rewritten when saved.

## Store damage

A record this version cannot read at all is store damage or a bug, not a state of the program. It is handled so that it fails nothing else, and this is where it shows:

* Pages report it in `skipped`, and asking for it by id answers `Error::Json`.
* A message whose body object has gone still lists; reading it answers `Error::BodyMissing`, and forward refuses with the same error instead of sending an empty mail. Deleting such a message works.
* `next_unread`, `mark_read`, the newest message of a conversation and threading go past it. An unreadable unread message stays counted in `unread_count`.
* A sync counts an `Error::Json` as `rejected` and goes on; a conversation that cannot be read is replaced by a new one for the same thread, so no mail is lost.
* `drafts()` and `accounts()` leave it out **without reporting it**. Their result is a plain list. A later version should report it there too.

An account that cannot be read stops its sync with `Error::Json`, because nothing can be said about it.

A record that lost its sort keys is neither listed nor reported.

## Known limits

* **The ciphertext attachment stays.** When a readable version replaces an undecrypted one, the attachments it brings are added, and what the undecrypted version had stays beside them, because the core cannot tell which of them is the ciphertext.
* **A stub stays a stub.** A later delivery of the same message that would fit, for example after the limit is raised, does not replace it. Replacing it would take two store steps, or give up the original (ADR 0030); that is a choice, not a limit of `fs-objects`. A real mail with a very long `To` (over 64 KiB, about 3000 addresses) therefore loses its content in this program for good; it stays on the server.
* **A stub keeps only the first 8 aliases.** A replay known only by a later one is stored again: a duplicate beats a lost mail.
* **A provider holding a repo clone keeps the claim on its store** for as long as it is registered, so the store cannot be opened again until then. It fails safe: the open is refused.
* **A crash can orphan a `Document`.** A body or attachment is stored before the message that names it. After a crash in between, an unreferenced object holds that text. It is encrypted at rest, and nothing removes it yet. The same holds for a crash between the tombstone and the release of a body when deleting.
* **A connector that fills `tool_calls` or `reasoning` from remote content must cap them itself.** They count towards the 25 MiB but not towards the 64 KiB for metadata, so a 5 MiB result adds 40 MiB of log over 4 changes (measured). Mail and Matrix fill neither.

## Decisions

* **Aliases have an authority (provisional, by the owner's review).** Only what a provider issued (UID, JMAP email id, Matrix event id) recognises a replay: `RawMessage.aliases`. What a sender chose (`Message-ID`) only links replies: `RawMessage.links`; a `message-id` in `aliases` is moved there, and a message with nothing else is refused. One exception: what a receipt names matches a returning copy of what we sent, and only if the copy's sender is the account's own address. **Consequence: the same mail in two IMAP folders shows twice unless the provider says otherwise. A duplicate beats a lost mail.**
* **Aliases hang on the account's identity,** kind plus normalised address, so removing and adding an account does not make its mail new, and a reply goes out over the account as it is now. Addresses compare without case or surrounding space. Provisional: the same mail in two accounts is two messages, and a sender in one account cannot hide a message in another.
* **Enforced in the core, not in providers.** The plan comes from the provider, the verdict from `gate.rs`. No provider, a provider without a plan, `none`, `transport-only`, `unknown`, or a good plan with a missing key: all refused, hidden recipients counted. A plan naming nobody blames all recipients.
* **The provider is not trusted further than it can be.** The core cannot read ciphertext. It can only refuse beforehand and report afterwards: a receipt weaker than end-to-end stores the message as reported and answers `EncryptionDowngraded`. A provider that lies, or sends on its own, is out of reach of this crate: connectors must be reviewed.
* **Agent input claims nothing.** `id`, `revision`, `risk`, `options`, `argumentsDigest`, `askedOf` are never taken from a report, `detail` moves into `summary` (contract: agent text goes there only). An agent's number is never compared to the engine's revision. An empty reference is refused, because a replay would add the call again.
* **An outside question is `working`.** It is shown and not asked: nobody here can answer it, and `refused` would claim something the agent never hears, while `settled` being final would hide what it does next. The contract wording is being corrected accordingly.
* **What a stranger claims about a reply goes, the mail stays.** A self-link is dropped, a link that would close a cycle is given up, a `|` or a control character in a sender-chosen id drops that id, and none of them reach an `Outgoing`, also from older records. The same in an alias or a conversation alias is refused by name, so connectors must not build those from sender text unencoded. A provider's receipt is checked the same way.
* **The first arrived wins as parent,** so a later message cannot take a thread over by claiming the same id.
* **A match on a receipt merges nothing.** It only says "this is my own copy", so what the copy calls itself is not learned from it. Only an alias the provider issued is believed for that, and for replacing an undecrypted version.
* **A conversation gives way only if this send created it,** when its thread turns out to be known already. A reply stays in its thread.
* **Scheme labels matter.** Only `message-id` (`MESSAGE_ID`) is recognised as sender-chosen. A connector that puts a sender's id under another label in `aliases` makes it dedupe.
* **`outgoing` on a delivered message:** a connector sets it from its own Sent folder only, and the core believes it only if the sender is the account's own address; otherwise it is an ordinary incoming mail.
* **A body type other than plain or markdown falls back to `text/plain`.**
* **A bad tool call goes alone:** one with an empty reference is dropped, the message stays. `record_tool_call` refuses it.
* **`Kind`, `DeliveryState`, `ToolCallState` have `unknown` as in the contract.** `ToolCallOutcome` has it here only, for a value this version cannot read; the server sends that as `open`, which the contract reads as "not known" on a settled call.
* **Delete = delete + tombstone.** The original cannot be removed by an update (immutable), so the object is deleted and an AS2 `Tombstone` under the same id keeps aliases and order.
* **A message gets its own id at sending, not the draft's**, because the store keeps `published` of an existing object and a message must sort by when it was sent. Its id stays through edits.
* **Provisional: message order.** Newest first by `published`, which the sender writes and can forge within the limits above. Page size 50, configurable: the contract has none.
* **Provisional: attachments** are store objects holding the bytes as their original; `Attachment.blob` is that id, to be wrapped in the contract's `Blob` capability by the server. They go when nothing names them. Forward shares them.
* **`pd:Communication`** is the sensitive category of everything, verified as existing in DPV 2.3. It stays for now. `fs-hub` withholds marked objects, see "Local only".

## Open points

Contract, for the owner:

1. `Compose.draft(replyTo)` has no account: with several accounts nothing says which one sends. The core adds `Draft::set_account` (provisional, refused on a reply); with exactly one account, it is taken.
2. `Draft.setSubject` is in `fs-interfaces` v0.27.0. `Draft::set_subject` mirrors the name with no dependency change; `Outgoing::subject` exposes it.
3. No page size, and no direction for `messages`.
4. `MessageSummary.arrival`, `undecrypted`, `signature`, `MessagePage.skipped` and `unknown` on `Kind`, `ToolCallState`, `DeliveryState` are in `fs-interfaces` v0.27.0. The core mirrors them; `skipped` is a count there and the ids here.
5. "Read all I see" (see "Reading").

Code:

0. The patterns of the guard's core files that are not in coding.md section 2 are listed in `crates/fs-messenger-guard/DESIGN.md`, open point 0. Other modules named `Specification`, `Memento`, `Null Object` or `Command` in the map above were not reviewed in this round.
1. **Send is not exactly-once.** A store failure right after a provider accepted answers an error and keeps the draft open, so a retry can send twice. A failed send stays visible, and its retry is a second message.
2. **Echo race.** A copy fetched while a send is under way, before the receipt is stored, is not recognised and is stored as a second message. Needs a provider-side client id.
3. **Delete takes two store steps;** a crash in between lets a replay revive it. The store's log keeps the old image, content included, until compaction; the program must call `compact()`, which only removes entries acknowledged by the hub. Since messages do not sync through the hub, nothing acknowledges them yet.
4. **Mark read** writes one transaction per message.
5. **Performance.** Pages load whole objects, originals included; search is a substring scan (`fs-objects` has no tokenised search), and body text is held twice for it, up to its limit. See "What a large body costs".
6. Removing an account keeps its messages, and those conversations can no longer send until the account comes back.
7. A claimed parent that never arrives stays pending.
8. The lag catch-up re-reads the log from the last seen id; on a read failure it waits for the next lag.
9. A forged `Message-ID` can still misthread a reply, as links are the sender's word. Harmless to delivery by design.

## Crates

Feature `test-support`: `MessengerRepo::open`, off unless a build asks for it.

`fs-objects` (git tag v0.3.0), `fs-interfaces` (git tag v0.27.0): `Capabilities`, `Unsupported`. `serde`, `serde_json`, `chrono`, `thiserror`, `async-trait`, `tokio`; dev: `tempfile`. No network crates in this crate. `scripts/test-servers` belongs to the connector work, not to this crate.

## v4 beside v6: review of 3fabd82 (Thread M, 2026-11-08)
Fixed: (1) a signature of the legacy v4 key verifies a mail but is no proof for an offer; only the held key in use proves (`Signers::proving`, `opened_mail`). (2) A first sight keeps the v6 key only; the v4 companion comes in later, with proof (`beside`). (5) A trailing `;` in an Autocrypt header is ignored. Open: (3) a peer who only sends `Older` mail is never learned as v6 and its legacy key never comes in with proof; (4) one v4-only reader puts the whole group on SEIPDv1, not yet shown to the person; (6) the real-GnuPG test (`interop_gnupg.rs`, deleted by this branch) is to be rewritten: import of our `keydata`, `gpg --decrypt` of an `Older` mail, signature check, a mixed group; and the Interoperability section above is out of date (it still says GnuPG cannot read our mail). No test yet for (1) by a forged mail signed with the legacy key.

## Accepted ideas, first steps (Thread M, 2026-11-08; `requirements.md`, "Accepted ideas")
Pure rules with tests, **now wired into the repository through `Messenger`** (capability ids of our own until `messenger.capnp` carries them: `receipt.choice`, `conversation.whyPlain`, `conversation.export`, `message.hold`). Nothing of it is in a frontend or in a provider's sending path yet:
- **4, read receipts both ways** (`receipts.rs`): `receipts_both_ways(mine, theirs)` is true only when both chose `Shown`; the default is `Hidden`. A provider sends and a frontend shows a receipt only if it is true. The choice per account is not stored yet. **Wired:** `Messenger::receipt_choice` / `set_receipt_choice` (a stored setting, default `Hidden`) and `show_receipt(theirs)`, which a provider asks before it sends a receipt and a frontend before it shows one. **Still open:** no provider sends a receipt at all yet. `receipt.choice` is `NeverForAi` in the guard's table: what a person shows of reading is theirs to say.
- **9, explain encryption** (`why_plain.rs`, `words.rs`): the reason is a value (`WhyNot`, `Remedy`) chosen by `why_not(Facts)`; the German sentences are in `words.rs` (ADR 0057). `transport-only`, `none` and `unknown` are never shown as encrypted. The facts (is the other side's key known, can the account encrypt) are the provider's to fill in. **Wired:** `Messenger::why_not_encrypted(conversation)` gives `Option<(WhyNot, Remedy)>` from the conversation's encryption and the accounts (`encryption_ready`); the other side counts as ready only if the conversation is already end-to-end encrypted.
- **10, export** (`export.rs`): **JSON Lines, one ActivityStreams 2.0 `Note` per message** (W3C Recommendation). Chosen because our envelope already is AS2 (ADR 0027), nothing is invented, any JSON reader works and a file of lines can be cut, appended and streamed. Mail keeps the original byte for byte in `original` (ADR 0030); **mbox (RFC 4155)** is what to ask a mail provider for and is not rebuilt here. Attachments are not in the file yet (names only would be a half answer); open. **Wired:** `Messenger::export_conversation(conversation)` pages through the conversation and answers the JSON Lines, oldest first; a message that cannot be read fails the export instead of being left out. Attachments appear by name, type and size (`attachment`), not their bytes. The whole file is built in memory (no streaming yet).
- **1, cool-down and recall** (`cooldown.rs`): `CoolDown::hold/release/send_now/read/recall`, default wait `COOL_DOWN_SECS` (600). A recall works while held or sent and unread. Whether a provider can really withdraw a sent message (Matrix redaction, mail: no) is the provider's to say; mail can only recall while held. **Wired:** `hold(draft, now, wait)`, `held_stage`, `send_held_now`, `recall_held`, `release_due(now)`: a held message is a stored record (`freesynergy.net:Held`) naming a saved draft and its `CoolDown`; the host calls `release_due` on a timer (the core has no clock or scheduler of its own). A failed send puts the message back to held, **except a downgrade, which means it left** (it stays sent, as for `Draft::send`). One message that cannot go (its draft was deleted, a key is missing) does not hold the others back; it is tried again at the next `release_due`, and the error comes out only when nothing else went. If the process dies between marking and sending, or the send comes back as a downgrade, the record stays "sent" without a sent message: `held_unsure()` lists such records, nothing sends them again by itself, and a person checks the sent folder. `recall_held` works only while held: after it went out no provider here can take it back (`Conflict`). Not built: recall of a sent, unread message by a provider that can (Matrix redaction).
