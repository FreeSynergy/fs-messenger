# fs-messenger-matrix – Design

The Matrix provider of FreeSynergy.Messenger: one account behind the core's `Provider` trait, over `matrix-sdk` 0.19.1 (pinned with `=`). No GUI, no RPC, no bridges.

## State

Version 0.4.0 (the workspace's), not tagged (v0.3.0 is the core's tag on main; matrix has none). Tested against the pinned tuwunel 1.9.4 only; see "Not verified".

| Area | State |
|---|---|
| Login with password, session restore, same device and identity after a restart | done, against the server |
| Stores: `CryptoStore` and `StateStore` over an encrypted `fs-store`, pure Rust, no SQLite | done: the SDK's own suites pass (35 and 28 tests; one more is `#[ignore]`d by the suite itself), restart against the server |
| Sync into the core: rooms are conversations, `m.room.message` are messages, room and event id are the alias (`EventAlias`), `m.in_reply_to` threads, cursor persists in the core, replay does not duplicate, own copy recognised | done, against the server |
| Gaps: a limited `/sync`, a newly joined room and a replay are read through `/messages`: at most 20 pages and 500 events per fetch, a page or a `/sync` answer over its limit cut and the rest kept as a gap; at most 8 open gaps per room, joined when they touch; see "What a server cannot do" | done, against the server for the ordinary case; the cuts only against a raw server |
| First reading of a room: a room counts as read (`cursor.rooms`) only through the cursor of a fetch that went through. After a failed fetch, or a cursor the core did not keep, the SDK stands ahead of the core; the replay then reads a room the cursor does not know from its start, not only down to the old position | done, tested against a raw server (three failure places) |
| Original: the event JSON as received; see "Originals" | done |
| Sending: `m.room.message` only into a room with `m.room.encryption`; plan and provider both check | done, against the server |
| Incoming marks: `megolm` only after real decryption; plain is `none`; undecrypted is `unknown` | done, against the server |
| Undecrypted, then the key arrives: replaced in place, same message, same original | done, against the server; see "Late keys" |
| Secrets: see "Secrets" | done, tested with needles from the real secrets |
| Review fixes (T2): URL with user info refused; alias holds the room; member cap in the plan; pattern names corrected; open points listed | done, tested; see "Open points" |
| A hostile server: the token stays at the configured origin and goes only to the server that issued it, requests end, memory is bounded where this crate can bound it; see "What a server cannot do" | done, tested; third review: the SDK's start-up task was not under the hard end, now it is |
| Second review (T2b): token bound to its homeserver; a plain event cannot take an undecrypted one's place; pages and events per fetch bounded; no password in URL errors or `Debug`; alias used for the waiting list and the echo; one end for a whole `send` and plan; patterns corrected | done, each with a test that failed before; offline against raw servers and units, not against tuwunel (the 13 server tests still pass); see "Not verified" |
| Third review (T2c): the SDK's start-up task (`wait_for_e2ee_initialization_tasks`, which goes to the network) now runs under `bounded` in `login`, `restore` and resume; a cut `/sync` plans one gap and moves the cursor on; gaps joined and capped at 8 per room; `receive/mod.rs` head corrected | done, each with a test that failed before (login and restore with a `429`, `retry_after` one hour; a raw server that cuts every sync); offline against raw servers, the 13 server tests still pass; see "Not verified" |
| Sixth review (T2e): a `403 M_FORBIDDEN` on `/messages` gives the room's gap up by name instead of blocking the account; banned rooms and left rooms the cursor does not know are not replayed after a lost answer; a leave timeline read down to the old position has a test | done: F1 and F2 against raw servers (both failed before); F3 test passed at once (it proves what was said); a ban and a forgotten room against tuwunel (answers recorded under "Not verified") |
| Fifth review (T2d): a return to a room is read as a new room; room ids with `|` or control characters reach the core (one encoding, `receive/ids.rs`); a gap in an unknown room is named when given up; pattern names and heads corrected (Adapter, not Facade) | done: F1 against tuwunel (the return test failed before), F2 and F4 against raw servers (failed before); see "Not verified" |

### `Unsupported`, or not built

| What | How it shows |
|---|---|
| Key verification, device list | `key.verify`, `device.list` reported unsupported; see "Device trust" |
| Sending files | `Unsupported("attachment.send")`, nothing leaves |
| Receiving files | `attachment.fetch` reported unsupported; the message shows `[m.image] name`, the original is kept |
| Hidden recipients | `Unsupported("message.bcc")`: a room has no hidden members |
| Push, long polling | `message.push` reported unsupported; call `fetch`, which does not wait |
| Key backup on the server, cross-signing | `key.backup` reported unsupported. `export_room_keys` and `import_room_keys` carry keys by file |
| Presence, calls, bridges | reported unsupported. The sync says `offline` |
| Reactions, redactions, receipts, typing | not read; a redaction does not reach a stored message, the core has no way in |
| Edits | not understood as edits: a message of its own, with the new text, answering its target. The core has no way to replace |
| Markdown | `text/markdown` goes out as plain text, no `formatted_body` |
| MLS, Telegram, GUI, RPC, a verification GUI, the SDK's send queue, sliding sync | not here |
| Multi-process use of one store | not supported: `CrossProcessLockConfig::SingleProcess` |

## Pattern map (ADR 0043)

Names are those of section 2 of `conventions/coding.md`, and only where the code is that. Where none fits, the module doc says `Pattern: none` and the place is under "Open points".

| Module | Pattern | Why |
|---|---|---|
| `provider/matrix_provider.rs` | Adapter | `MatrixProvider` wraps the SDK's `Client` and implements the core's `Provider` over it |
| `provider/{fetch,send,convert,history,connect,rooms}.rs` | Adapter, part of `MatrixProvider` | the same type, split by topic: sync, sending, events to messages, `/messages`, login, the provider's own room methods |
| `store/crypto/*`, `store/state/*` (`decorate`, `store`) | Decorator | the SDK's memory stores, with a journal in front |
| `store/forward.rs` | Decorator, the plain half | nearly a hundred methods only pass on |
| `store/journal.rs` | Strategy | the `Journal` trait: the stores do not know `fs-store` |
| `store/fs_journal.rs` | Adapter | `FsJournal` implements `Journal` over `fs-store` |
| `secret.rs` | Newtype | `Password`, `SecretText`: a secret cannot be printed |
| `receive/alias.rs` | Value object | `EventAlias`: small, private fields, compared by value, written to and read from one text without loss |
| `receive/cursor.rs` | Memento | `MatrixCursor`: a snapshot of where the provider stands; the core keeps it as opaque text and hands it back |
| `provider/plan.rs` | Specification | `Verdict::passes` (the gate "encrypted only") and `within_member_limit`: pure predicates. `weakest` picks the weaker of two `Encryption` values and is no predicate; nothing combines predicates ("combinable" of the table does not apply). The SDK queries around them are methods of `MatrixProvider` (Adapter, part of it) |
| `provider/account.rs` | Specification | `check_homeserver`: a pure predicate over the URL; all its refusals have to hold. (`MatrixAccount` itself is plain data with public fields: no Value object) |
| `lib.rs`, `provider/mod.rs`, `receive/mod.rs`, `store/mod.rs`, `store/{crypto,state}/mod.rs` | none | only declare modules and re-export (section 3): no logic |
| `receive/ids.rs` | none, allowed (small group of pure functions) | the one encoding of a server id inside a core id: `escape`, `unescape`, `conversation_of`, `room_of` |
| `receive/{adapter,event,gaps}.rs` | none, allowed (small group of pure functions) | a Matrix event to a `RawMessage`; reading an event's JSON; planning and merging gaps |
| `store/{crypto,state}/persist.rs`, `store/{keys,hex,read_back}.rs` | none, allowed (small group of pure functions) | a change as journal entries; journal keys; hex; reading records back |
| `store/{crypto,state}/restore.rs` | Decorator, part of `FsCryptoStore` / `FsStateStore` | opening the decorator rebuilds the wrapped memory store from the journal |
| `error.rs`, `*/kind.rs` | none | an error enum, constants |

No pattern of section 2 fits `provider/deadline.rs` and `provider/echo.rs`, and they are not a group of pure functions: they have behaviour. Their module heads say `Pattern: none, but not allowed by section 2`, and both stand under "Open points". Head, this map and the open points say the same.

## Stores: pure Rust, and how

`matrix-sdk` 0.19.1 with default features brings `matrix-sdk-sqlite` (rusqlite, libsqlite3-sys: C) and aws-lc (C). Both are off. Choice (a) of the assignment holds: its `StateStore` and `CryptoStore` traits are implemented over `fs-store`, in about 1500 lines, inline tests included, against the 6100 of the SDK's SQLite versions of those two.

**Decorator over the SDK's own memory stores.** Reads come from `MemoryStore`, so all its query logic is the SDK's. Every write goes first to a journal, then to memory. Opening replays the journal into a new memory store.

**One `save_changes` is one object**, written by `put_unlogged` in one transaction, so it is all or nothing. After 512 segments the journal folds into a base segment, and whatever lies before the newest base is ignored.

**Why `put_unlogged`.** A logged `put` keeps the old image in the activity log until compaction, and compaction only removes what the hub acknowledged. For a crypto store that would keep every old ratchet state on disk. **Limits:** `fs-store` says that old pages can stay in the file until reused, so old states may linger inside the encrypted file. A value is wiped from RAM as a `SecretText`, but the plain `serde_json::Value` of a segment on its way to the store, and the SDK's memory store, are not.

**Everything is in RAM** while the provider runs: the SDK's memory stores hold it all, as `MemoryStore` was built to. Fine for one person's account; not measured at scale. Folding reads everything.

**At rest.** The journal refuses a store for which `is_encrypted` is false, also one that opted out of the sensitive-data rule. Every segment carries the sensitive category. There is no SDK passphrase: the key comes from `fs-store`'s `KeyProvider`, and pickles have no key of their own. **A program should use a store of its own for this**, not synchronised to a hub.

**Not kept across a restart, on purpose:** presence, receipts, global profiles, composer drafts, recently visited rooms, the UTD hook's data and the server capability caches (the SDK fills them again or this crate does not use them); the send queue and thread subscriptions (not used). The event cache and media store are the SDK's in-memory defaults.

**C code that remains** in the normal tree, none of it from these stores:

| Crate | What | Comes through |
|---|---|---|
| `ring` | C and assembly | `rustls`, the TLS of `reqwest` (also in `fs-messenger-mail`) |
| `blake3` | C and assembly, on x86-64 unless its `pure` feature is on | `matrix-sdk-store-encryption`, a dependency of `matrix-sdk-base` |
| `simsimd` | C | `turso_core`, so already in `fs-store` and in the core |

`aegis` (through `turso_core`) is built with its `pure-rust` feature (`cargo tree -e features`), so its C is not compiled, though `cc` is listed for it. No `libsqlite3-sys`, `rusqlite`, `aws-lc`, `openssl-sys` or `native-tls` in `cargo tree -e normal`.

## Secrets (blocker before merge)

No access token, password, passphrase, Olm or Megolm key in an activity, a hub path, `Debug`, `Display`, an error or a log line. There is no recovery key: no backup, no recovery. There is no store passphrase of the SDK: the key comes from `fs-store`'s `KeyProvider`, and this crate never holds it.

* **Own redacting types.** `Password`, and `SecretText` for every journal value, whatever it holds, so no code decides which one is secret. No `Display`, `Debug` prints a placeholder, memory is wiped on drop, readable only inside this crate. `Debug` of `MatrixProvider` shows ids, and does not ask the client.
* **Errors.** A `serde_json` message quotes what it read: `invalid type: string "..."`. So errors give the kind and position only, and a pickle that fails says which and not why. Tested with a damaged record.
* **Activity and hub.** All stores write through `put_unlogged`: nothing in the activity log, no change-feed event, nothing the hub reads. The core's own log holds the account and its cursor, whose waiting list holds **ciphertext**, never a key.
* **The test** (`tests/server/secrets.rs`) takes needles from the real secrets: the token from the store, a real Megolm session key from a decrypted export, a piece of the real Olm pickle, the password, the passphrase. It looks in `Debug`, in errors, in every log line of every crate at TRACE, in the whole activity log of both stores through the store's API, in what the hub reads, and in the raw files. The Olm needle is a text piece of the pickle: a log printing the same bytes in another form would pass it.
* **It refuses an unencrypted store**, also one that opted out and an in-memory one, before anything is sent. Tested at the provider.

## What a server cannot do

Found in the review of the mail crate, and all three were true here, by the SDK's defaults:

* **Move the token.** The SDK follows `well_known` in a login answer, and `reqwest` follows redirects. Both are off: `respect_login_well_known(false)` and `redirect::Policy::none()`. **A homeserver behind a redirect does not work**; configure its real URL. **The token also goes only to the server that issued it:** the session is stored with the homeserver's origin (scheme, host, port; the path does not count), and a `restore` or `login` over a stored session for another origin fails before the first request. A session stored without an origin (an earlier draft) is refused the same way: use a new store and log in again. Tested with a raw server that must hear nothing. Both tests fail with the protection removed; tried.
* **Hold the provider up.** Read in the SDK and tried here, each case on a test: without a retry limit it retries `5xx` and `429` for ever; a network failure or a timeout it does not retry without one. **With a limit it still waits as long as a `429` names in `retry_after`**, so a server could hold it for an hour. So three things: a time limit per request and a stall limit per read; 3 tries; and `MatrixAccount::bounded`, a hard end of 8 times `request_timeout` (30 s, so 4 minutes) around each call into the SDK that goes to the network: `login`, `restore` and resume included, through the SDK's start-up task in `started()` (a call there past the end is an error and no provider is handed out; a stored session stays, so `restore` can try again). **Not under it, because they do not use the network** (read in the SDK 0.19.1): `restore_session`, `Room::decrypt_event`, `get_user_devices`, `get_dm_rooms`, `ed25519_key`, the key export and import (files), the state store. All provisional. **Dropping a send at that end can leave the message sent and recorded as failed**, so a retry sends it twice, as with any lost answer (core's open point 1).
* **Fill memory, as far as this crate can tell.** `limit` is only a request, so the answer is cut, not trusted. Through `/messages`: pages of 100 asked for, at most 20 pages and 500 events per fetch. A page with more events than that is cut at the budget and its rest is read from the same token in the next fetch (`Gap::skip`; a server that changes a page in between can make that miss or repeat events, the core drops repeats). A `/sync` room with more events than `timeline_limit` keeps its newest `timeline_limit`, the cursor moves on to the answer's `next_batch`, and one gap from there down to where we were is planned for that room (events delivered before come again, the core drops them). The first cut with nothing open says `more` and reads the gap in the next fetch; a cut with gaps open already does not say `more`, so a server that cuts every answer cannot keep `sync()` going. **Reaching the event budget says `more`** (real progress). **Reaching the page limit first does not**: small pages are what a server that never ends looks like, so the fetch ends with the gap in the cursor and `sync()` of the core returns; the next call goes on. A user with more than 20 rooms with gaps may need a second `sync()`. **Gaps are bounded: at most 8 per room.** A new gap that ends where an open one starts joins it; past 8 the two oldest become one (newest start, oldest end), which covers both and what lies between, so some already read part may be read again and the core drops it. **Nothing is given up**: a hostile server leaves a gap open for ever, and the gaps of one room do not grow with it. **The cursor as a whole still grows with invented rooms**: `cursor.rooms` takes every room id a server invents, and each such room may hold up to 8 gaps (open points, "Without a total end" (d)). An empty page that carries an `end` does not end a gap (CS API v1.16: paginate until no `end` comes); at most 8 such empty pages are followed per fetch, then the fetch ends with the gap open in the cursor, and the next one goes on from the newest `end`. A gap of a hostile server that only ever sends empty pages therefore never ends, as with endless full pages (open points, "Limits not set" (b)); it can keep other gaps behind it waiting, since the one gap read last in the list is read first. This rests on a room's gaps being planned oldest first, which they are. **A server that serves endless full pages still keeps one `sync()` of the core busy**, as each page is progress; the core's loop has no limit (wanted from core, 10). The waiting list holds 256 events and 1 MiB; an event over the specification's 64 KiB does not wait. **Not bounded: the size of one HTTP answer.** The SDK reads it whole and offers no limit, and the core's 25 MiB stub only helps after that; only the time limit stands in the way. The number of rooms in one `/sync` is not bounded either. Wanted from the SDK.

## Originals (ADR 0030)

The SDK hands over only the cleartext of what it decrypted. So:

* **Decrypted:** `GET /rooms/{room}/event/{id}` per message, one more request each, and that answer's bytes are the original. It holds `room_id`, which `/sync` leaves out. A failure fails the fetch, and the next one replays.
* **Plain and undecrypted:** the bytes as `/sync` or `/messages` gave them.
* `unsigned.age` differs in every answer of a server. So "byte for byte" means: as it was received, once.
* The **cleartext** of a decrypted event is **not kept**, only the text in `body_text`: the core has one original. A better converter later can read the ciphertext only with keys. Wanted from core.

## Sending and the two checks

The core's plan only sees recipients, and a reply's recipient is the parent's sender, not the room. So a reply into a plain room passes the plan if that person has keys. **The provider checks the room itself in `send`**, and nothing reaches `Room::send` otherwise. Tested on the server. The core then records the message as `failed` and answers `EncryptionRefused`.

A recipient is a room (`!...`) or a person (`@...`). A person without a room gets a new encrypted one when the message goes out, with an invitation; the plan answers from their devices. Several people get one new room. **Invitations are never accepted on their own** (provisional): anyone can send one, and joining shows them our devices. `invites` and `accept_invite` are the provider's own, since the core has no such concept.

`missing_keys` names a room if the room is the problem (plain, not joined, an unknown algorithm), and a person if that person has no usable device.

## Late keys: what actually happens

The SDK allows trying again: `Room::decrypt_event`. The core replaces in place: a known provider-issued alias with `undecrypted` unset, on a message with it set. So:

1. A message without a key is stored with body `UNDECRYPTED_MARK`, `undecrypted` set, encryption `unknown`, its original kept. **`unknown` and not `none`:** it is known to be encrypted, and not known to be readable to whom. Neither reads as protected. The core shows no preview for it and does not find it by search.
2. The event waits in the cursor, so the list moves with the batch. At most 256; an older one stays a placeholder for good.
3. Every fetch tries again, without the network. What opens goes to the core under **the same event id**. Same message id, same original, `megolm`, the text in place. It counts as `duplicates` in a `SyncReport`, not as `stored`.

**A plain `m.room.message` under the id of an event that waits as a placeholder (same room) is not delivered.** The core would replace the placeholder by alias, and the text would be the sender's, `undecrypted` unset, the real key later dropped. It is dropped and named in a `tracing` warning (ids only); the placeholder stays. The core has no way yet to be told (wanted from core, 9). Only what is on the cursor's waiting list is known: a placeholder that left the list (more than 256, or over 1 MiB) is not, and nothing here can see the core's journal. The core still replaces by alias alone (wanted from core, 8).

The same happens if a replay finds the event readable. Encrypted events of a type other than `m.room.message` also leave a placeholder, as nobody can tell before decrypting; when they open, nothing follows, so **such a placeholder stays**, and it leaves the waiting list. Provisional.

## Device trust (PROVISIONAL)

What the SDK does by default, unchanged:

* **Outgoing:** `CollectStrategy::AllDevices`. Keys go to **every device of every member that is not blacklisted, unverified ones included**, as in Element X's legacy mode. MSC4153 advises against this. A malicious homeserver that adds a device to a member gets the keys. The plan says `megolm` for this.
* **Incoming:** `TrustRequirement::Untrusted`: an unverified sender decrypts and is `megolm`. **`megolm` means "decrypted", not "the sender is who it says".** `RawMessage.signature` stays at `not verified`, and `EncryptionInfo.verification_state` is not used.
* Cross-signing is not set up. Nobody here can be verified: `key.verify` and `device.list` are `Unsupported`.

## Decisions

* **Provisional: schemes.** `matrix` event id, `matrix-room` room id.
* **The mark goes by the event.** A plain event in an encrypted room is `none`.
* **The SDK's position, not the core's, is what `/sync` asks from.** The SDK drops an answer whose `next_batch` it has seen. Asking from an older cursor would answer nothing, and the cursor would move past the batch. So the difference comes from `/messages`. A server test caught this.
* **What a newly joined room said before the join** is read once, by `/messages`. The list of rooms read is in the cursor. **Only rooms the account is in count as read:** after every sync a room that is left, rejected, banned or forgotten is taken out of the list, so that a return is read as a new room (down to the start, as far as the server shows it), not only down to the old position. Duplicates are the core's to drop. Before this, a message said while the account was away was never delivered after the return (fifth review F1, shown against tuwunel). Messages up to the leave are read first, if the server gives them: the gaps of the room stay open and are read as long as the SDK knows the room (test `review_left_room`, raw server: a limited `leave` timeline makes a gap down to the old position, `to=`, and the messages after it arrive). **A lost answer replays only rooms worth it** (sixth review F2, `worth_replaying` in `fetch.rs`): joined rooms; left rooms only while the cursor still knows them (the fetch that took the leave read down to the leave and took the room out of the list, so a retry no longer reads it from its start); invited, knocked and banned rooms never. Chosen over a list of left rooms in the cursor because it needs no new state: the list would only have kept the one case below. Cost: a room joined and left again entirely inside lost answers (three failed fetches in a row, the SDK having taken each answer) is not replayed; what it said before the leave is lost. Banned rooms: a ban that comes with a limited timeline still plans its gap from `/sync` (messages up to the ban are shown by the server, see below); a `403` there is given up by name.
* **The room id in a core id** goes through `receive/ids.rs` only (`%`, `/`, `|`, control characters as `%XX`): the alias of an event and the conversation of a room. Before, the conversation was the bare room id, and the core refused `!a|b:x` (`rejected`, the cursor went on, the message was gone). A room id without those characters is unchanged, so earlier stored conversations stay the same.
* **A gap that cannot be read** (the SDK does not know the room: left and forgotten, or the SDK's store was lost while the core's cursor stayed) is not removed in silence: it moves to `given_up` in the cursor (room and tokens, the newest 32) and a `tracing` warning names it. The core cannot show it (wanted from core, 9).
* **Relations are in the clear.** The SDK sends `m.relates_to` outside the ciphertext, as the spec says: **the server sees which message answers which**, not the text.
* **The alias holds the room.** `matrix` + `<room>/<event>`, with `%`, `/`, `|` and control characters in either part written `%XX` (the core refuses `|` and control characters in an id: the first version used `|` and the replay server test caught it) (`EventAlias`). The core replaces a placeholder by alias without comparing conversation or sender, so with the event id alone a hostile homeserver could send an event with room A's placeholder id into room B and have A's placeholder overwritten (sender Alice, text Mallory's, `megolm`). Now such an event has another alias. **In the same room** the alias is the same, so this does not help: see "Late keys". **Incompatible with aliases of earlier drafts** (`matrix` + bare event id): stored messages would be seen as new on a replay, and an old placeholder is not replaced. The cursor holds no alias (room and event id are separate fields), so it stays readable. A reply parent counts only if its alias names the room being sent into.
* **Own echo:** sending and syncing exclude each other, and what this process sent is not delivered back. After a restart the core's receipt recognises it. `Message.receipts` holds the event id; `aliases` only if a copy did come by.
* **The `/sync` filter** sets the timeline limit to 100 per room, configurable, sent inline.
* **`http://` only to loopback.** The test server has no TLS. The homeserver URL is parsed by the `url` crate: user info (also with `https`), a backslash and an unparsable URL are refused; plain `http` needs the host exactly `localhost`, `127.0.0.1` or `[::1]`. Before, `http://localhost:80@evil.org` passed.
* **Rooms over 500 members** are not checked person by person (each may cost a request): `send` and the plan fail closed with an error. Provisional.

## Wanted from core

Worked around here, nothing changed there:

1. **Replacing a readable message** (`RawMessage.replaces`, or an `Ingest` method). Only an undecrypted one can be replaced today. Without it: edits and redactions.
2. **`encryption_plan` should get the conversation.** It only sees recipients: see "Sending and the two checks". Today a reply into a plain room can pass the plan and end in `failed`.
3. **A second original or a cleartext field**, so that what was decrypted can be derived anew.
4. **The sender's verification state.** `RawMessage.signature` now exists, and stays unset here: `not verified`. It speaks of signatures, and whether a verified Megolm sender is `Valid` is for the core to say.
5. **Rooms in the model:** invitations, joining.
6. **`Outgoing` could carry the client's own id** (transaction id), which would close the echo race for all providers.
7. **Replace by alias should compare the conversation** (and the sender) of the placeholder with the new message. This crate defends itself by putting the room into the alias, other providers do not.
8. **Replace a placeholder only by the decrypted variant of the same ciphertext** (`repo/ingest_upgrade.rs`): today any message with the alias and `undecrypted` unset replaces it, also a plain one from the same room. This crate drops such an event while the placeholder is on its waiting list (see "Late keys"), which does not cover a placeholder that left the list.
9. **A way to report what a provider dropped or gave up** (a `dropped` count or list in `FetchBatch`, shown in `SyncReport`): now only a `tracing` warning and, for given-up gaps, the cursor's `given_up` list; an error would hold the account up for ever. The core's `SyncReport.rejected` counts what the core refused (`Invalid`) after the fetch; the provider cannot see it, and the cursor goes past those messages. The encoding of room ids is tested to give well-formed ids, so this crate no longer causes it for room ids; a refusal for another reason (a sender, a size) is counted by the core and named nowhere (room and event id would help).
10. **A limit for one `sync()`** (events, pages or time): the loop in `repo/ingest.rs` goes on while `more` and the cursor changes, so a server of endless full pages keeps it busy; this crate can only end a fetch without `more`.

## Open points (for the strategy session)

**Patterns (fifth review, F5; section 2 as changed on 2026-11-07).** Every file was compared with the definitions of the table. `MatrixProvider` implements `Provider` over `matrix-sdk`: that is an Adapter, not a Facade, and the `impl` files (`fetch`, `send`, `convert`, `history`, `connect`, `rooms`) say `Adapter, part of MatrixProvider`. `FsCryptoStore`/`FsStateStore` `restore.rs` are parts of the Decorator. `receive/alias.rs` is a Value object, `receive/cursor.rs` a Memento, `provider/plan.rs` and `provider/account.rs` Specifications (only where the code is a predicate), `receive/gaps.rs`, `adapter.rs`, `event.rs`, `ids.rs`, `store/*/persist.rs`, `keys`, `hex`, `read_back` are small groups of pure functions for one concept (`none`, allowed). **No pattern fits, and the file has behaviour (reported to the strategy session):** `provider/deadline.rs` (`MatrixAccount::bounded` and `whole`: a time limit around a future. It is not pure and implements no trait, so it is not a Decorator) and `provider/echo.rs` (`SentEvents`: a bounded, mutable set of sent ids under a lock; a Value object is immutable, so not that). Judgement calls to check: `store/crypto/restore.rs` and `persist.rs` are the longest of the "small groups" (about 190 and 140 lines with tests), `receive/adapter.rs` is a conversion to a type of ours, which the table's Adapter example (import) would also cover; it is named `none` because the code wraps no foreign type and implements no trait. `receive/cursor.rs` is a Memento with some logic of its own (`wait_for`, `give_up`).

**A permanent failure blocks the account (changed for gaps, not for `ciphertext_of`).** Sixth review F1: a `403` with `M_FORBIDDEN` on `/messages` (both, read from the SDK's error as `MatrixError::Forbidden`, not from text) gives that room's gap up by name: room and tokens in `given_up` of the cursor (newest 32), a `warn` in the log, never content; the other rooms go on. Every other error (5xx, network, `429`, a 5xx that borrows the code) fails the fetch and keeps the gap. **Not changed:** `ciphertext_of` (convert) still passes a 403/404 for one event on, so that one event fails every fetch for ever; a decrypted event whose ciphertext cannot be fetched has no ciphertext to wait with, so its placeholder would stay for good. A given-up gap loses messages without the core being told (no "gap" concept in the core; wanted from core). Which errors count as permanent is a decision: only `M_FORBIDDEN` with 403 so far.

**Limits not set (partly changed).** (a) A deadline for a whole fetch: dropping `fetch_batch` half-way is safe by design (the replay), but a fetch that always exceeds it would never progress; the value is a decision. (b) Pages per gap across fetches: a hostile server can return endless pages with ever-new tokens. Within one fetch 20 pages and 500 events bound the work, a fetch cut by the page limit ends without `more`, and the gaps of a room are capped at 8 (joined, never dropped), so the cursor stays small and `sync()` returns. Across calls such a gap never ends: giving it up would lose history without the core being told (it has no concept of it; wanted from core, 9 and 10 cover the report and the limit), so it is not done.

**Without a total end (partly changed).** One `send` and one plan now have an end of 16 times `request_timeout` (8 minutes at 30 s); passing it is an error and nothing counts as sent (as with `bounded`, the message may have left all the same). Still open: (a) inside that end, a room of up to 500 members is checked person by person, each up to 4 minutes (at worst about 33 hours without the new end; the new end cuts it, so a large room whose server is slow fails instead of finishing). (b) The member cap counts members, not devices: a few members with very many devices pass it, and only the 4-minute end per call and the new total stand in the way, not memory. (c) Each decrypted event takes one more request, `ciphertext_of` (up to 4 minutes), so a fetch of 500 decrypted events from a slow server can take hours; there is no end for a whole fetch (above, "Limits not set" (a)). (d) `cursor.rooms` grows with every room id a server invents, and the cursor is stored with each batch; the number of rooms in one `/sync` is not bounded (above). Gaps are capped per room, not in all, so invented rooms still make the list longer (8 each at most). A cap on `cursor.rooms` was not added: dropping a room from it makes the room read from its start again, and with more real rooms than the cap that would repeat for ever; the cap needs a named task (what happens to the room beyond it) and is for the strategy session.

## Not verified

* **No other Matrix client.** Both ends are this crate, plus plain events by HTTP. Element or any other client has not read these messages or written to this crate.
* **TLS.** Only plain HTTP to loopback ran. The `https` path with `ring` as provider has never connected to anything.
* **Federation** is off on the test server, and only tuwunel ran; Synapse did not.
* **Crash safety** is by design, not by test: no process was killed during a write or a fold. The replay after "the SDK saw it, the core did not" is tested by asking with an old cursor.
* **Key forwarding between own devices** (`automatic-room-key-forwarding` is on): not seen working. The SDK only answers verified devices, and nothing here can verify. The late-key test brings its key by export file.
* **An expired or revoked access token:** not tried. It should show as a `matrix:` error; there is no flow to log in again over a stored session. Refresh tokens are not requested.
* **Concurrent send and sync:** the exclusion is by a lock and the test does them in turn; no test forces the race.
* **Scale:** rooms of two or three, tens of messages. Each journal write took about 20 ms here (900 writes in 18 s in the fold test, on this machine), and a sync can make several.
* **The second review's corrections** (stored origin, the dropped plain event, page and event limits, the `/sync` cut, password-free errors, alias keys, the total end) were tested offline: raw HTTP servers that answer what each case needs, units, and the core on an encrypted file. None of them against tuwunel: the 13 server tests still pass, and none of them reaches a limit. The `/sync` cut and the page cut rest on the assumption that a server answers `/messages` from a `/sync` token with the same events when asked twice; the raw server does, a real one was not tried.
* **The third review's corrections** (the start-up task under `bounded`, the cut `/sync`, joined and capped gaps) were tested offline: a raw server answering `429` with `retry_after` of an hour on `/account_data`, for login and restore, and a raw server that cuts every sync and pages without end (gaps, cursor size, `sync()` of the core). The 13 server tests pass, and none reaches a cut. Whether a real server answers `/messages` from a `next_batch` token as from `prev_batch` is assumed, as before. The merged gap relies on the order a room's gaps are planned in; a unit test covers it, no server does.
* **The total end of `send` and the plan** is tested as a unit (a future that never ends); no test drives a real slow room.
* **A plain event under a waiting placeholder's id** is tested with one waiting placeholder. A placeholder that already left the waiting list is not covered.
* **Edits and files** are tested only on hand-written JSON.
* **MPL-2.0** comes along with the SDK: `as_variant`, `eyeball`, `eyeball-im`, `imbl`, `imbl-sized-chunks`, `readlock`, `readlock-tokio`. File-level copyleft, not our code; not an ADR 0015 question I can settle.

* **The fourth review's corrections** (empty pages with an `end`, the first reading of a room after a failed fetch) were tested against a raw server only (`review_empty_pages`, `review_room_first_read`): failure of `/messages` on the first and on a later page, and a cursor the caller did not keep. A failure in `ciphertext_of` or in the conversion is not driven by a test (a raw server cannot make the SDK decrypt); by the code, a failed fetch drops its local cursor like the others, and the replay now treats the room the same way. The empty-page cap (8 per fetch) is a guess. The 13 server tests pass; none reaches an empty page.

* **The fifth review's corrections.** F1 (return to a room): `tests/server/rejoin.rs` against tuwunel 1.9.4: leave, a message while away, invitation, join (failed before the fix), and two guards that also passed before it on this server: leave and return between two syncs, and a refused invitation. Not tried: another server software (Synapse may show a leave and join in one `/sync` differently: then the room is `known` and the old position is the bound), banning, `/forget` (the SDK may keep or drop the room; the list is pruned by SDK state in both cases), a second account's list (it is per account, in its own cursor; nothing shared). F2: raw server only (`review_room_id`); the real server was not asked to make a room id with `|` (tuwunel makes ids of its own). F4: raw server, a cursor written by hand with a gap for an unknown room; a lost SDK store was not reproduced against tuwunel.

## Tests

`cargo test -p fs-messenger-matrix`: offline, unit tests, the SDK's two conformance suites over our stores and `store_restart`, all on encrypted files. The suites read from memory, so they prove little about persistence alone; `store_restart` and the server tests do that.

`hostile_server` runs the real provider against a small raw HTTP server that misbehaves on purpose; that server is no stand-in for Matrix, and no claim about Matrix rests on it.

The server tests are `#[ignore]`d; `tests/server/main.rs` says how to run them. No test double: real provider, real core, real server, and what the server holds read by plain HTTP.

## Crates

`matrix-sdk`, `matrix-sdk-base` (`=0.19.1`, Apache-2.0), `rustls` with ring (TLS, as in the mail crate). `matrix_sdk::crypto` is not re-exported, hence `matrix-sdk-base`. `fs-store`, `fs-interfaces`, `fs-messenger-core`, `async-trait`, `chrono`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `zeroize`.

* **The sixth review's corrections.** F1/F2: raw servers (`review_banned`, `review_left_room`), failed before the fix. F3: `review_left_room` (a limited `leave` timeline, gap to `to=<old position>`) passed against the unchanged code, so the statement was true but unproved until now; it is proved against a raw server only. **Against tuwunel 1.9.4** (`tests/server/banned.rs`): a banned account's own `/messages` answers `200` and shows the room up to the ban (history visibility `shared`), not what came after; a room the account left and **forgot** answers **`403 M_FORBIDDEN`** ("You don't have permission to view this room"), and the provider's syncs go on. So the 403 path is real on this server for forgotten rooms, but this crate's own give-up was only driven by a raw server: no test made the real server answer 403 to a gap the provider had planned (whether the SDK keeps a forgotten room, and so plans a gap for it, was not looked at). **Synapse was not asked**: the report's claim that it answers 403 for banned or forgotten users is not checked. Not tried: a ban with a limited timeline against the real server.
