# fs-messenger-matrix – Design

The Matrix provider of FreeSynergy.Messenger: one account behind the core's `Provider` trait, over `matrix-sdk` 0.19.1 (pinned with `=`). No GUI, no RPC, no bridges.

## State

Version 0.1.0, not tagged. Tested against the pinned tuwunel 1.9.4 only; see "Not verified".

| Area | State |
|---|---|
| Login with password, session restore, same device and identity after a restart | done, against the server |
| Stores: `CryptoStore` and `StateStore` over an encrypted `fs-store`, pure Rust, no SQLite | done: the SDK's own suites pass (35 and 28 tests; one more is `#[ignore]`d by the suite itself), restart against the server |
| Sync into the core: rooms are conversations, `m.room.message` are messages, event id is the alias, `m.in_reply_to` threads, cursor persists in the core, replay does not duplicate, own copy recognised | done, against the server |
| Gaps: a limited `/sync`, a newly joined room and a replay are read through `/messages`, 500 events per fetch, going on in the next (`more`) | done, against the server |
| Original: the event JSON as received; see "Originals" | done |
| Sending: `m.room.message` only into a room with `m.room.encryption`; plan and provider both check | done, against the server |
| Incoming marks: `megolm` only after real decryption; plain is `none`; undecrypted is `unknown` | done, against the server |
| Undecrypted, then the key arrives: replaced in place, same message, same original | done, against the server; see "Late keys" |
| Secrets: see "Secrets" | done, tested with needles from the real secrets |
| A hostile server: the token stays at the configured origin, requests end, memory is bounded where this crate can bound it; see "What a server cannot do" | done, tested |

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

| Module | Pattern | Why |
|---|---|---|
| `provider/facade.rs` | Facade + Strategy | the core sees `Provider`; behind it the SDK client |
| `provider/connect.rs` | Factory | client over our stores; a stored session wins over a password |
| `provider/account.rs` | Specification | plain `http://` only to loopback: a token would be readable on the way |
| `provider/fetch.rs` | Template method | sync, close gaps, convert, retry, say where we are |
| `provider/convert.rs` | Adapter | one event to one message; the mark comes from how the SDK came by the text |
| `provider/history.rs` | Iterator | the pages of `/messages` |
| `provider/plan.rs` | Specification | is there really an encrypted way; used by the plan and by `send` |
| `provider/send.rs` | Template method | find the room, check again, send, report what happened |
| `provider/echo.rs` | Value object | closes the core's echo race from this side |
| `provider/rooms.rs` | Facade | invitations, opening rooms, key files: the core has no room concept |
| `receive/*` | Adapter, Specification | pure: JSON to `RawMessage`, no network |
| `receive/cursor.rs` | Value object | the token and the waiting list move together |
| `store/crypto/*`, `store/state/*` | Decorator | the SDK's memory stores, with a journal in front |
| `store/forward.rs` | Decorator, the plain half | nearly a hundred methods only pass on |
| `store/journal.rs`, `store/fs_journal.rs` | Strategy, Adapter | atomic log of changes, over `fs-store` |
| `store/keys.rs`, `store/hex.rs`, `store/read_back.rs` | Strategy (pure functions) | unambiguous keys, whatever a server puts in them |
| `secret.rs` | Newtype | `Password`, `SecretText`: a secret cannot be printed |

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

* **Move the token.** The SDK follows `well_known` in a login answer, and `reqwest` follows redirects. Both are off: `respect_login_well_known(false)` and `redirect::Policy::none()`. **A homeserver behind a redirect does not work**; configure its real URL. Both tests fail with the protection removed; tried.
* **Hold the provider up.** Read in the SDK and tried here, each case on a test: without a retry limit it retries `5xx` and `429` for ever; a network failure or a timeout it does not retry without one. **With a limit it still waits as long as a `429` names in `retry_after`**, so a server could hold it for an hour. So three things: a time limit per request and a stall limit per read; 3 tries; and `MatrixAccount::bounded`, a hard end of 8 times `request_timeout` (30 s, so 4 minutes) around each call into the SDK that goes to the network. All provisional. **Dropping a send at that end can leave the message sent and recorded as failed**, so a retry sends it twice, as with any lost answer (core's open point 1).
* **Fill memory, as far as this crate can tell.** 500 events per fetch through `/messages`, going on in the next; the waiting list holds 256 events and 1 MiB; an event over the specification's 64 KiB does not wait. **Not bounded: the size of one HTTP answer.** The SDK reads it whole and offers no limit, and the core's 25 MiB stub only helps after that; only the time limit stands in the way. The number of rooms in one `/sync` is not bounded either. Wanted from the SDK.

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
* **Own echo:** sending and syncing exclude each other, and what this process sent is not delivered back. After a restart the core's receipt recognises it. `Message.receipts` holds the event id; `aliases` only if a copy did come by.
* **The `/sync` filter** sets the timeline limit to 100 per room, configurable, sent inline.
* **`http://` only to loopback.** The test server has no TLS.

## Wanted from core

Worked around here, nothing changed there:

1. **Replacing a readable message** (`RawMessage.replaces`, or an `Ingest` method). Only an undecrypted one can be replaced today. Without it: edits and redactions.
2. **`encryption_plan` should get the conversation.** It only sees recipients: see "Sending and the two checks". Today a reply into a plain room can pass the plan and end in `failed`.
3. **A second original or a cleartext field**, so that what was decrypted can be derived anew.
4. **The sender's verification state.** `RawMessage.signature` now exists, and stays unset here: `not verified`. It speaks of signatures, and whether a verified Megolm sender is `Valid` is for the core to say.
5. **Rooms in the model:** invitations, joining.
6. **`Outgoing` could carry the client's own id** (transaction id), which would close the echo race for all providers.

## Not verified

* **No other Matrix client.** Both ends are this crate, plus plain events by HTTP. Element or any other client has not read these messages or written to this crate.
* **TLS.** Only plain HTTP to loopback ran. The `https` path with `ring` as provider has never connected to anything.
* **Federation** is off on the test server, and only tuwunel ran; Synapse did not.
* **Crash safety** is by design, not by test: no process was killed during a write or a fold. The replay after "the SDK saw it, the core did not" is tested by asking with an old cursor.
* **Key forwarding between own devices** (`automatic-room-key-forwarding` is on): not seen working. The SDK only answers verified devices, and nothing here can verify. The late-key test brings its key by export file.
* **An expired or revoked access token:** not tried. It should show as a `matrix:` error; there is no flow to log in again over a stored session. Refresh tokens are not requested.
* **Concurrent send and sync:** the exclusion is by a lock and the test does them in turn; no test forces the race.
* **Scale:** rooms of two or three, tens of messages. Each journal write took about 20 ms here (900 writes in 18 s in the fold test, on this machine), and a sync can make several.
* **Edits and files** are tested only on hand-written JSON.
* **MPL-2.0** comes along with the SDK: `as_variant`, `eyeball`, `eyeball-im`, `imbl`, `imbl-sized-chunks`, `readlock`, `readlock-tokio`. File-level copyleft, not our code; not an ADR 0015 question I can settle.

## Tests

`cargo test -p fs-messenger-matrix`: offline, unit tests, the SDK's two conformance suites over our stores and `store_restart`, all on encrypted files. The suites read from memory, so they prove little about persistence alone; `store_restart` and the server tests do that.

`hostile_server` runs the real provider against a small raw HTTP server that misbehaves on purpose; that server is no stand-in for Matrix, and no claim about Matrix rests on it.

The server tests are `#[ignore]`d; `tests/server/main.rs` says how to run them. No test double: real provider, real core, real server, and what the server holds read by plain HTTP.

## Crates

`matrix-sdk`, `matrix-sdk-base` (`=0.19.1`, Apache-2.0), `rustls` with ring (TLS, as in the mail crate). `matrix_sdk::crypto` is not re-exported, hence `matrix-sdk-base`. `fs-store`, `fs-interfaces`, `fs-messenger-core`, `async-trait`, `chrono`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `zeroize`.
