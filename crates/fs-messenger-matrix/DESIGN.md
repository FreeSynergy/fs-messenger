# fs-messenger-matrix – Design

The Matrix provider of FreeSynergy.Messenger: one account behind the core's `Provider` trait, over `matrix-sdk` 0.19.1 (pinned with `=`). No GUI, no RPC, no bridges.

## State

Version 0.1.1 (the workspace's), not tagged. Tested against the pinned tuwunel 1.9.4 only; see "Not verified".

| Area | State |
|---|---|
| Login with password, session restore, same device and identity after a restart | done, against the server |
| Stores: `CryptoStore` and `StateStore` over an encrypted `fs-store`, pure Rust, no SQLite | done: the SDK's own suites pass (35 and 28 tests; one more is `#[ignore]`d by the suite itself), restart against the server |
| Sync into the core: rooms are conversations, `m.room.message` are messages, room and event id are the alias (`EventAlias`), `m.in_reply_to` threads, cursor persists in the core, replay does not duplicate, own copy recognised | done, against the server |
| Gaps: a limited `/sync`, a newly joined room and a replay are read through `/messages`: at most 20 pages and 500 events per fetch, a page or a `/sync` answer over its limit cut and the rest kept; see "What a server cannot do" | done, against the server for the ordinary case; the cuts only against a raw server |
| Original: the event JSON as received; see "Originals" | done |
| Sending: `m.room.message` only into a room with `m.room.encryption`; plan and provider both check | done, against the server |
| Incoming marks: `megolm` only after real decryption; plain is `none`; undecrypted is `unknown` | done, against the server |
| Undecrypted, then the key arrives: replaced in place, same message, same original | done, against the server; see "Late keys" |
| Secrets: see "Secrets" | done, tested with needles from the real secrets |
| Review fixes (T2): URL with user info refused; alias holds the room; member cap in the plan; pattern names corrected; open points listed | done, tested; see "Open points" |
| A hostile server: the token stays at the configured origin and goes only to the server that issued it, requests end, memory is bounded where this crate can bound it; see "What a server cannot do" | done, tested |
| Second review (T2b): token bound to its homeserver; a plain event cannot take an undecrypted one's place; pages and events per fetch bounded; no password in URL errors or `Debug`; alias used for the waiting list and the echo; one end for a whole `send` and plan; patterns corrected | done, each with a test that failed before; offline against raw servers and units, not against tuwunel (the 13 server tests still pass); see "Not verified" |

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
| `provider/facade.rs` | Facade + Strategy | the core sees `Provider`; behind it the SDK client |
| `store/crypto/*`, `store/state/*` (`decorate`, `store`) | Decorator | the SDK's memory stores, with a journal in front |
| `store/forward.rs` | Decorator, the plain half | nearly a hundred methods only pass on |
| `store/journal.rs` | Strategy | the `Journal` trait: the stores do not know `fs-store` |
| `store/fs_journal.rs` | Adapter | `FsJournal` implements `Journal` over `fs-store` |
| `secret.rs` | Newtype | `Password`, `SecretText`: a secret cannot be printed |
| `lib.rs`, `provider/mod.rs` | none | only declare modules and re-export (coding rules, section 3): no logic, so no Facade |
| `error.rs`, `*/kind.rs` | none | an error enum, constants |

No pattern of section 2 fits these (module doc: `Pattern: none`), see "Open points": `provider/connect.rs` (builds a concrete client, no trait object: not a Factory), `provider/rooms.rs` (more methods of `MatrixProvider`, not a type: not a Facade), `provider/deadline.rs`, `receive/alias.rs` (`EventAlias`: two public fields, no invariant: not a Newtype), `provider/account.rs`, `provider/fetch.rs`, `provider/send.rs`, `provider/convert.rs`, `provider/history.rs`, `provider/plan.rs`, `provider/echo.rs`, `receive/{mod,adapter,event,cursor}.rs`, `store/crypto/{persist,restore}.rs`, `store/state/{persist,restore}.rs`, `store/{keys,hex,read_back}.rs`.

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
* **Hold the provider up.** Read in the SDK and tried here, each case on a test: without a retry limit it retries `5xx` and `429` for ever; a network failure or a timeout it does not retry without one. **With a limit it still waits as long as a `429` names in `retry_after`**, so a server could hold it for an hour. So three things: a time limit per request and a stall limit per read; 3 tries; and `MatrixAccount::bounded`, a hard end of 8 times `request_timeout` (30 s, so 4 minutes) around each call into the SDK that goes to the network. All provisional. **Dropping a send at that end can leave the message sent and recorded as failed**, so a retry sends it twice, as with any lost answer (core's open point 1).
* **Fill memory, as far as this crate can tell.** `limit` is only a request, so the answer is cut, not trusted. Through `/messages`: pages of 100 asked for, at most 20 pages and 500 events per fetch. A page with more events than that is cut at the budget and its rest is read from the same token in the next fetch (`Gap::skip`; a server that changes a page in between can make that miss or repeat events, the core drops repeats). A `/sync` room with more events than `timeline_limit` keeps its newest `timeline_limit`, and the cursor keeps its old `since`, so the next fetch reads the whole difference through `/messages` (events delivered before come again, the core drops them). **Reaching the event budget says `more`** (real progress). **Reaching the page limit first does not**: small pages are what a server that never ends looks like, so the fetch ends with the gap in the cursor and `sync()` of the core returns; the next call goes on. A user with more than 20 rooms with gaps may need a second `sync()`. **A server that serves endless full pages still keeps one `sync()` of the core busy**, as each page is progress; the core's loop has no limit (wanted from core, 10). The waiting list holds 256 events and 1 MiB; an event over the specification's 64 KiB does not wait. **Not bounded: the size of one HTTP answer.** The SDK reads it whole and offers no limit, and the core's 25 MiB stub only helps after that; only the time limit stands in the way. The number of rooms in one `/sync` is not bounded either. Wanted from the SDK.

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
* **What a newly joined room said before the join** is read once, by `/messages`. The list of rooms read is in the cursor.
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
9. **A way to report events a provider dropped** (a `dropped` count or list in `FetchBatch`, shown in `SyncReport`): now only a `tracing` warning, and an error would hold the account up for ever.
10. **A limit for one `sync()`** (events, pages or time): the loop in `repo/ingest.rs` goes on while `more` and the cursor changes, so a server of endless full pages keeps it busy; this crate can only end a fetch without `more`.

## Open points (for the strategy session)

**Patterns: "an Strategie melden".** The code is free functions and plain structs; section 2 of the coding rules has no pattern for these, and none is invented here. The old names were wrong (no default trait methods for "Template method", no trait implemented for "Adapter", no `impl Iterator`, no trait for "Strategy", "Specification" and "Value object" are not in section 2): `provider/account.rs` (validation), `provider/fetch.rs`, `provider/send.rs` (sequences of steps), `provider/convert.rs`, `receive/{mod,adapter}.rs`, `store/{crypto,state}/{persist,restore}.rs` (conversion functions), `provider/history.rs` (resumable paging, no `impl Iterator`), `provider/plan.rs` (a check), `provider/echo.rs`, `receive/cursor.rs`, `receive/event.rs` (plain values and pure rules), `store/{keys,hex,read_back}.rs` (pure functions). Do these need a pattern in section 2, or is "none" right?

**A permanent failure blocks the account (not changed).** `ciphertext_of` (convert) and `room.messages` (history) pass their error on, so a 403/404 for one event or one gap fails every fetch for ever (the cursor stays). Retrying a bounded number of times is no problem, but the end of it is: to go on, the event must become a placeholder (`unknown`) or the gap be given up. A decrypted event whose ciphertext cannot be fetched has no ciphertext to wait with, so its placeholder would stay for good and its readable text be lost; a given-up gap loses messages without the core being told. Telling a permanent from a temporary failure also means reading the SDK's HTTP error kinds. That is a decision about losing data and about a "gap" concept in the core: not taken here.

**Limits not set (not changed).** (a) A deadline for a whole fetch: dropping `fetch_batch` half-way is safe by design (the replay), but a fetch that always exceeds it would never progress; the value is a decision. (b) A cap on pages per gap across fetches: a hostile server can return endless pages with ever-new tokens. Within one fetch 20 pages and 500 events now bound the work (an empty page counts as one event), and a fetch cut by the page limit ends without `more`, so one `sync()` returns and the gap stays in the cursor. Across calls the gap never ends. A cap would give up history, and the count needs a new cursor field: not taken here.

**Without a total end (partly changed).** One `send` and one plan now have an end of 16 times `request_timeout` (8 minutes at 30 s); passing it is an error and nothing counts as sent (as with `bounded`, the message may have left all the same). Still open: (a) inside that end, a room of up to 500 members is checked person by person, each up to 4 minutes (at worst about 33 hours without the new end; the new end cuts it, so a large room whose server is slow fails instead of finishing). (b) The member cap counts members, not devices: a few members with very many devices pass it, and only the 4-minute end per call and the new total stand in the way, not memory. (c) Each decrypted event takes one more request, `ciphertext_of` (up to 4 minutes), so a fetch of 500 decrypted events from a slow server can take hours; there is no end for a whole fetch (above, "Limits not set" (a)). (d) `cursor.rooms` and `cursor.gaps` grow with every room id a server invents, and the cursor is stored with each batch; the number of rooms in one `/sync` is not bounded (above). A new `Gap` per room is read at most 20 pages per fetch.

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
* **The total end of `send` and the plan** is tested as a unit (a future that never ends); no test drives a real slow room.
* **A plain event under a waiting placeholder's id** is tested with one waiting placeholder. A placeholder that already left the waiting list is not covered.
* **Edits and files** are tested only on hand-written JSON.
* **MPL-2.0** comes along with the SDK: `as_variant`, `eyeball`, `eyeball-im`, `imbl`, `imbl-sized-chunks`, `readlock`, `readlock-tokio`. File-level copyleft, not our code; not an ADR 0015 question I can settle.

## Tests

`cargo test -p fs-messenger-matrix`: offline, unit tests, the SDK's two conformance suites over our stores and `store_restart`, all on encrypted files. The suites read from memory, so they prove little about persistence alone; `store_restart` and the server tests do that.

`hostile_server` runs the real provider against a small raw HTTP server that misbehaves on purpose; that server is no stand-in for Matrix, and no claim about Matrix rests on it.

The server tests are `#[ignore]`d; `tests/server/main.rs` says how to run them. No test double: real provider, real core, real server, and what the server holds read by plain HTTP.

## Crates

`matrix-sdk`, `matrix-sdk-base` (`=0.19.1`, Apache-2.0), `rustls` with ring (TLS, as in the mail crate). `matrix_sdk::crypto` is not re-exported, hence `matrix-sdk-base`. `fs-store`, `fs-interfaces`, `fs-messenger-core`, `async-trait`, `chrono`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `zeroize`.
