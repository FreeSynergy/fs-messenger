# fs-messenger-guard – Design

The AI guard of FreeSynergy.Messenger: invitation, release, brake, emergency stop (ADR 0054, 0056). In the engine, below every frontend, without network and without a model. When in doubt it stays shut. Names mirror `messenger.capnp` draft 0.4 (branch `msg-guard` of `fs-interfaces`, not tagged); no generated types are used.

## State

| Area | State |
|---|---|
| `Gate::check` (`Allow`, `Ask`, `Refuse`), `revise`, `finished`, `admit_message`, as a pure state machine over a repository | done |
| Risk table per capability, one entry per `Conversation.act` member; what is not in it is `unknown`; the guard and `toolcall.decide` are refused for an AI outright | done, **provisional form** |
| Invitation by a signed `invite`; releases start empty; who reads as an AI by the rule below | done, **rule provisional** |
| `release` per participant and class; `revoke`; the brake, also against a tampered record | done |
| `decide`, routed from `Messenger::act`; every refusal listed in the contract, typed by the four `Problem.type` URIs; `toolCall` event on stale and settled | done |
| Device proof: `DeviceVerifier`, Ed25519 by `ed25519-compact`; signed bytes below | done, **key type provisional** |
| Local enrolment, first key by trust on first use | done, **provisional** |
| Emergency stop: latch, persist, settle, providers, log. Read-through fail-shut. `reopen` | done |
| `StopPermission`: `OwnerOnly`. Per group: `Unsupported` | done for one owner |
| Activity log without secrets, out of reach of undo | done |
| `sender_kind`: `SenderLookup` over the guard's invited list; the only way the core learns who is an AI. No account kind `agent` (removed 2026-11-07: an account belongs to a person, an AI has its own account in its own program) | done, **provisional**: `fs-people` pins `fs-store` v0.2.0, this repo v0.3.0, so it cannot be a dependency |
| Review of 2026-11-07: three races of the stop, each reproduced by a test first and then fixed: (1) a stop behind a slow `reopen` gave up on the lock after 4 s and the `reopen` then cleared marker, latch and mark (`tests/stop_wins_reopen.rs`); (2) between the end of that `reopen` and the stop's write `check` read "Open" and the way in for agents let a message through (same file); (3) `Allow` came after slow steps (log, save, display) with the last look before them, for a class release and for a used approval (`tests/stop_last_look.rs`). Fixed with a stop count, a clear that is atomic with its check, a marker put back by the retry task, and a last look right before `Allow` | done |
| Reports after a stop: from an `ai` or `unknown` sender they are kept `cancelled` (F1 of the second review); a panic in the judging of a stop writes the marker (F7); the put-back of the marker in `reopen` has a time limit (F8). Each reproduced by a test that failed first | done |
| Review of 2026-11-07, third: (1) the judging of a stop's proof has a time limit, `GuardConfig::judge_wait` (2 s), and runs verifier and permission on a thread of its own; past it, or on a panic, the stop is by nobody (`tests/stop_judge_hangs.rs`); (2) the retry task makes each try in a task of its own, so a panicking save no longer ends the retries (`tests/stop_retry_panics.rs`); (3) a report is checked a second time after its write (core `tests/report_after_stop.rs`). Each reproduced by a test that failed first | done |
| Ratings, trust levels through third parties, MCP, ACP, RPC, GUI, model providers | **not built**; see below |

### Not built

| What | How it shows |
|---|---|
| A stop per group | `Guard::stop_group` and `StopPermission::may_stop_group` answer `Unsupported("guard.stop.group")`. ADR 0054 point 5 says it is a normal permission setting: if someone gives everyone the right, everyone may. Groups come with `fs-groups` |
| Trust levels 0 to 3, ratings, operators | nothing here reads them: only invitation and release count. ADR 0054 point 6 leaves the basis open |
| A `Messenger.guard` method (contract `guard @8`) | deliberately **not** on the Rust trait, so no path leads there; the RPC layer composes it for a person's device |
| MCP, `input_required`, `requestState`, ACP | a tool layer calls `Gate`; `Verdict::Ask` has what it needs. Nothing untested is claimed |
| Replacing a lost last key while the state is intact | no path: AIs stay shut out, the rest of the messenger works. Open point 3 |
| AIs in conversations (ADR 0054, "KIs in Unterhaltungen") | nothing of it: a notice before an AI joins; all members must agree, else it stays out; a later member accepts it or stays out; consent withdrawn means the AI must leave; no history from before it joined; no exception for local AIs. The guard knows invited AIs, not conversations' consent |
| Who is calling, proven | `Caller` is built by the tool layer that does not exist yet; ADR 0054 suggests the system user over a Unix socket |

### Tests

Against real, encrypted `TursoStore`s in temp files, restarted by dropping every handle and opening the files again. A hand-held clock moves expiry. Only three kinds of test double exist, and each only adds a failure or a behaviour around the real thing: a repository whose writes fail or wait, over the real one; providers that hang, panic or cannot stop; and a guard that does nothing, to reach the port alone. Each guarantee was checked by removing it from a clone and seeing a test fail.

## State table

Program (`GuardMode`):

| From | Event | To | Rule |
|---|---|---|---|
| none / missing | anything | reads `stopped` | fresh program; nothing is written by reading |
| any | unreadable, corrupt, unknown version | reads `stopped` | not written; `damage()` says why |
| `stopped` | `reopen`, permitted proof, written | `open` | not written: stays `stopped`. Releases stay taken, also stored ones |
| `open` | `stop`, permitted proof | `stopped` | latch first; a failed write is reported, stays in memory, and is tried again |
| corrupt | `LocalEnrolment::recover_damaged` | `stopped`, empty | new engine identity: older proofs no longer fit |

Call (`CallStage`, shown as `ToolCallState`):

| From | Event | To |
|---|---|---|
| none | `check` needs a person | `question` |
| `question` | `allowOnce` / `allowClass` | `approved` (shown `working`), for one use |
| `question` | `refuse` | settled `refused` |
| `question`, `approved` | `revise` with other input | `question`, new digest, new revision |
| `approved` | `check` with exactly these arguments | `running` (shown `working`) |
| `running` | `finished` | settled `done` / `failed` |
| not settled | stop, `Conversation.stop`, `revoke` | settled `cancelled` |
| `question`, `approved` | expiry, swept by the next new question | settled `cancelled` |
| settled | anything | stays |

## Pattern map (ADR 0043)

Only patterns of coding.md section 2. Where none fits it says `none`, and open point 0 lists it for the strategy.

| Module | Pattern | Trait / form | Why |
|---|---|---|---|
| `api.rs` | Strategy | traits `Gate`, `Guard` | a tool layer holds `dyn Gate` and has no `Guard` method |
| `state/stage.rs`, `state/calls.rs`, `state/decide.rs`, `state/admin.rs`, `state/doc.rs` | State | enums `CallStage`, `GuardMode` and pure transitions | all rules on a copy; the engine writes the whole result or nothing |
| `state/proof.rs`, `risk.rs`, `digest.rs`, `identity.rs`, `signed.rs`, `model.rs`, `error.rs`, `log.rs`, `enrol.rs`, `engine/marker.rs`, `lib.rs` | none | free functions or plain types | open point 0 |
| `repo/traits.rs`, `repo/turso.rs` | Repository, Adapter | trait `GuardRepository`; `TursoGuardRepository` over `TursoStore` | one opaque record, one log; whatever comes back is checked |
| `engine/*` (`core`, `gate`, `decide`, `questions`, `stop*`, `admin`) | Facade | `GuardEngine`, its `impl` blocks split by concept | fixed order of each operation, which is the guarantee; no trait with provided methods, so no Template method |
| `engine/store_access.rs` | Facade | time limits and fail-shut around `GuardRepository` | not a Decorator: the trait is not implemented again |
| `engine/wiring.rs` | Adapter | `Route` implements the core's `EngineGuard` and `SenderLookup` over the engine | the core knows the traits, not this crate; no cycle |
| `table.rs` | Registry | `TABLE`: capability to rule | becomes schema annotations |
| `arguments.rs` | Newtype | `Arguments` | exists only canonical, unique and within the limit |
| `device.rs`, `permission.rs`, `clock.rs` | Strategy | `DeviceVerifier`, `StopPermission`, `Clock` | key type, who may pull it, and time can change |
| core: `guard_hook.rs` | Strategy | `EngineGuard`, `SenderLookup` | who is an AI is the strategy `SenderLookup`; `open_for_reports` is how the core learns that the program is shut |
| core: `repo/guard_port.rs`, `repo/guard_turns.rs` | Facade | `GuardPort` | one port, handed out once; providers asked in tasks of their own |
| core: `repo/guard_note.rs`, `repo/tool_calls.rs` | Adapter (and Strategy in `tool_calls.rs`) | engine call to `Note`; agent report to `ToolCall` | a report can claim nothing; shut guard means `cancelled` |
| core: `provider/turn.rs`, `model/tool_call.rs`, `repo/message_index.rs`, `repo/capabilities.rs` | none | plain data types and pure functions | open point 0 |

## Decisions

**Signed bytes (fixed).** Every field is a 4-byte big-endian length and its bytes; the arguments have a 4-byte count in front:

```
field("FreeSynergy.Messenger guard device proof v1") field(engine identity) field(method) field(signer)
count(arguments) field(argument)... field(nonce)
```

`method` is `invite`, `release`, `revoke`, `uninvite`, `stop`, `reopen`, `decide`, `addDevice`, `removeDevice`. Arguments: `invite`, `revoke`, `uninvite`: profile. `release`: profile, risk by its contract name. `stop`, `reopen`: none. `decide`: message id, tool call id, option id, digest, revision as 8 big-endian bytes, nonce empty: `ToolCallDecision` has no nonce. `addDevice`, `removeDevice`: person, key in lowercase hex. The signer is a field beyond what the contract lists, so that a key shared by two persons cannot be replayed between them. The engine identity is 32 random bytes in the state: `GuardEngine::engine_identity`.

**Digest.** SHA-256 over fields, each written as its length (8 bytes, big-endian) and then its bytes, in this order:

```
field("FreeSynergy.Messenger tool call digest v1") field(caller)
field("no conversation")  or  field("conversation") field(conversation id)
field(tool name)
field("no input")  or  field("input") field(canonical arguments)
```

`caller` is the profile as the engine stores it (`id_key`: trimmed, lower case). The conversation id is the turn's conversation as text, a lower-case hyphenated UUID; without a conversation the fixed field `no conversation` stands in its place, so an `allowOnce` given in one conversation cannot be spent in another. The result is the 32 bytes; the record keeps it as lower-case hex and `ToolCall.argumentsDigest` carries the bytes. A device recomputes it from these fields and the canonical form below, nothing else. The canonical form is ours, not RFC 8785: keys sorted by bytes at every level, no white space, values as `serde_json` writes them. `1` and `1.0` differ, which at worst asks again. Keys are sorted by hand, because `serde_json`'s `preserve_order` can be turned on by any crate in a build. A duplicate key is refused: two programs can read it differently.

**Crates.** `ed25519-compact` 2.6 (pure Rust, MIT) with default features off. `ed25519-dalek` and `curve25519-dalek` are BSD-3-Clause, so they do not meet the rule of MIT or Apache-2.0. `sha2` and `getrandom`: MIT OR Apache-2.0.

**Single use.** A `GuardProof` is single use by signer and nonce, across all methods, 16 to 64 bytes. A decision by its signed bytes. Never by the signature's bytes: another valid signature over the same bytes exists, and is tested. Nothing is used up before the signature holds, so a forged proof cannot burn a nonce. Stored in the state, for good.

**One document, unlogged.** Mode, keys, invitations, releases, calls and used proofs are one object, written with `UnloggedObjects`, so a decision, its used proof and its release are atomic, and there is no before image that undo could restore. Every operation reads it through, so a record gone bad is seen at once. Arguments are not stored, only the digest. Cost, not measured: up to one JSON parse per `check`, and a used proof adds about 70 bytes for good.

**What is shown decides nothing.** A `ToolCall` in a message is a display of the guard's record. A question has no agent reference and no alias, so no report or delivery can name it. Undo can bring back what a message shows; a click on it is refused by the record, and the `toolCall` event of that refusal says how the call really stands. Engine writes use the actor `urn:freesynergy:messenger:guard`, so a person's own undo does not even reach them.

**Refused decisions.** `Error::DecisionRefused` carries `DecisionProblem`, with `type_uri()` as in the contract: `decisionStale` (digest or revision no longer match; also a call already answered and not over, since what was shown has moved on), `decisionSettled`, `decisionNotAsked`, `decisionInvalid` (everything else, also an id that is no call at this place, a stopped program with a call still open, and a damaged state). A stopped program answers `decisionSettled` for its cancelled calls, as that is what a card needs. On stale and settled a `toolCall` event follows, from the guard's record, over an in-memory channel beside the store feed: nothing is written, not even to the log. It goes out before the signature is checked, and says nothing that the subscription does not already show. A listener more than 64 announcements behind misses it and reads the message. Failures to write are `Error::GuardRefused`, not a decision problem.

**Who is an AI (provisional).** An AI is a participant, not a kind of account. Whether a sender is one is asked through `SenderLookup::sender_kind`: `ai` only for a profile the person invited, else `unknown`; on no answer in time, an unreadable state, or no guard, also `unknown`. Never `person`, and a sender string gives no right: rights go by `Caller`. What counts is the person's invitation list, never what a provider or the AI says of itself. Every message is stored, whatever the sender, also while stopped: a stored message is no action, and mail is not lost. A forged sender can claim `ai` or stay `unknown`, and gets nothing from either. KI = Teilnehmer; the source of the mark will later be `fs-people` (`kind` application); for now provisionally the guard's invited list. Also provisional: the profile is compared with the transport's sender string, where the contract speaks of card ids. A report of a tool call gets no right, whoever sends it. While the program is not open (stopped, damaged, or the guard does not answer), a report whose message was sent by an `ai` or `unknown` sender is stored as `settled`, `cancelled`: `EngineGuard::open_for_reports` asks the engine, and the core asks it under its write lock, so a stop either settles the report with the rest or has shut the guard before it lands. The core waits for that answer up to 1 second under its write lock; the guard never calls the core back from it, so there is no deadlock, and no answer reads as shut. The settling of a stop reads the index before it takes the write lock, so it can miss a message whose first report is being written at that moment; therefore the core asks the guard a second time after the write and cancels the report if the guard has shut meanwhile (a stop's latch is up before its settling begins: either the settling saw the message, or the second look sees the latch). `tests/report_after_stop.rs` in the core. `person` and `software` senders are not affected, and nothing is asked when no guard is installed. Before the stop, and after a `reopen`, a report is `working` as before (`tests/stop_late_report.rs`; core: `tests/report_after_stop.rs`).

**Retries (provisional numbers).** A bare `input_required` can be retried at once. Per question: not sooner than 2 seconds (`RetryTooSoon`), at most 60 retries (`RetriesExhausted`). Both refuse without writing, and the question stays open for the person; an approved call always goes through. Per participant: at most 16 open questions (`TooManyQuestions`), and the same arguments reuse the same question. The counter is in memory, so a restart starts it afresh. A question lives 10 minutes, an unused `allowOnce` 2 minutes. Arguments over 64 KiB are refused: what is shown is never cut off, so what cannot be shown whole cannot be asked about. Characters that hide or reorder text are shown escaped, and refused in names.

**Who may do what.** `invite`: any enrolled person, and that person is who gets asked. `release`, `revoke`, `uninvite`: who invited it, or the owner. `stop`, `reopen`: `StopPermission`; `OwnerOnly` takes the person who owns the program, from `GuardConfig::owner`. Pulling is ADR 0054's permission setting. A signer marked as AI is refused before any key is looked at, and before `StopPermission`, so no implementation can let one through (ADR 0056). `invite` and `release` are refused while stopped. A stop on a state that cannot be read has no key to check against: it needs no proof there, as it can only end work.

**First key (provisional).** `GuardEngine::open` hands back `LocalEnrolment` and nothing else can make one. The program keeps it with its local console. Whoever holds it at the first start puts in the owner's key: trust on first use, on the local machine. After that a key needs a proof by a key of the owner. An invited AI gets no key, and a person with a key cannot be invited. `recover_damaged` only works on a state that was read and is not one; never because the store did not answer.

**A stop beats a reopen that began earlier.** `reopen` takes a stop count when it is asked for. Latch, mark and marker go only if no stop came in since, checked and cleared in one step under the mark's lock; otherwise nothing is cleared, the marker is put back, and `reopen` answers `Stopped`. A stop that waits for the lock never gives up silently: the retry task writes marker and store as soon as the lock is free. The latch is never down between a stop's arrival and its write.

**Order of a stop.** Who pulls it, latch, write, settle what is shown, providers, log. `check` sees the latch without a lock. Providers run in tasks of their own with 5 seconds together; one that hangs or panics reports `NoAnswer`. The latch is not set again under the lock any more (since 86adc2b): it goes up once, at the start (`stopped.store(true)` only in `store_access.rs`, `mark_unsaved`), and a `reopen` clears it only through `clear_stop_unless_newer`, which compares the stop count and clears in one step under the mark's lock. A `reopen` that began first therefore cannot swallow the stop. The judging of the proof runs in a task of its own, within `judge_wait`: a verifier or permission that panics there, or does not answer in time, makes it a stop by nobody, with marker and retry as for any stop (`tests/stop_verifier_panics.rs`). Before 2026-11-07 such a panic left the latch up in memory only, so a restart lost the stop when the store still said "open".

**Own store.** Give the guard a store of its own, as `UnloggedObjects` advises. The rules also hold on a store shared with the messenger, and that worst case is what `tests/out_of_reach.rs` tries, but `fs-hub` pushes a store's log, so a shared store would send profile and tool names along. The repository refuses a store that is not encrypted.

## What cannot be enforced within one process

Plainly: **Rust privacy is no boundary against code in the same process.** A provider or plugin running in this process can read memory, open the store file again, or simply act without asking. Whoever can write the store can set the mode to `open`, add a key, or clear used proofs, and nothing here can detect it, because any key to detect it with would lie beside the record. What is built holds against every path through the `Messenger`, `Ingest` and `Provider` traits, not against code that leaves them.

That is why ADR 0056 requires **agents in their own sandbox under their own system user**: the store file and its key are out of reach of the agent because the operating system says so, not this crate. Connectors that run in the engine must be reviewed. The stop also only reaches what runs through our programs (ADR 0054): an AI with its own shell is outside it, and a provider that cannot end a turn is reported, not ended.

Also: `Verdict::Allow` is a snapshot. What has started is not called back, except through the provider's `stop_turns`. A write that fails during a stop is lost if the program dies before a later write works, and the report says so. The clock comes from the system: set back, it keeps a question alive for longer.

## Open points

0. **Patterns outside coding.md section 2 (to the strategy).** No pattern is invented. Where nothing in section 2 fits, the module header says `Pattern: none`. These are: `state/proof.rs` (pure checks on `GuardDoc`; was called Specification), `risk.rs` (pure rules; Specification), `digest.rs` (free functions; was Strategy), `identity.rs` (one function; was Strategy), `signed.rs` (free functions; was Builder, but there is no `XBuilder::build`), `model.rs` (plain data; was Value objects), `error.rs` (enums), `log.rs` (plain data; was Adapter), `enrol.rs` (a type that only `open` hands out; was Capability object), `engine/marker.rs` (file functions; was Memento), `lib.rs` (declarations only), and in the core `provider/turn.rs` (plain data; was Value object). Also in the core `repo/guard_port.rs`: that only one port exists is a convention without a section 2 name; the type itself is a Facade. `repo/guard_turns.rs` was called Template method, but is a method that fans out over the `Provider` strategy: now Facade. Doubtful, left as the nearest: `engine/*` as Facade (one `GuardEngine` split into files; `signed_change` in `admin.rs` is a helper function with a closure), `state/doc.rs` as State (a record with a mode). In the core, too: `model/tool_call.rs` (plain data; was Value objects), `repo/message_index.rs` (pure function; was Strategy), `repo/capabilities.rs` (a function filling a report, no `build()`; was Builder). Missing from section 2, and wanted by the code: Specification, Value object, Capability object, Memento.

1. The table's form, and its classes, are a proposal for the owner: `account.add` and `rule.write` as `actsForPerson` are my reading.
2. `CONTRACT_VERSION` stays `0.3` until draft 0.4 is tagged, though `ToolCallDecision.revision` and the problem types already mirror 0.4.
3. A lost last key, and the same question for groups: who may step in without opening a way for an AI?
4. Used proofs are kept for good. A proof with a time in it would allow dropping them; the contract has none.
5. Refused attempts are not logged: a refusal changes nothing, and a caller could otherwise fill the log. A person might want to see them; that needs a limit first.
6. A message with an open call stored before this version has no `open_call` index, so a stop does not settle what it shows. The guard's own record is not affected.
7. `Gate::admit_message` is not called by a tool layer yet; the engine uses the same rule (`admission`) for `EngineGuard::open_for_reports`, where "not invited" counts as open (a report gets no right either way). `admit_message` itself stays for a tool layer; remove it if none wants it.
8. A report on a message whose stored `sender_kind` is `unknown` or `ai` is cancelled while shut, also when the message is an outgoing one of the person's own (`send_record.rs` stores `unknown`). Harmless (a report on it is an AI's work), but it is why the rule goes by the stored kind and not by who reported: the contract's `recordToolCall` does not say who reports.
9. A hanging disk: the marker work runs on the runtime's blocking threads. `tests/reopen_stalled_disk.rs` stalls them all and shows that `reopen` still ends. A `stop` writes the marker with a time limit too, and the retry task goes on; what a disk that never comes back leaves is the stated limit above.
