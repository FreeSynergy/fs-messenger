# fs-messenger-guard – Design

The AI guard of FreeSynergy.Messenger: invitation, release, brake, emergency stop (ADR 0054, 0056). In the engine, below every frontend, without network and without a model. When in doubt it stays shut. Names mirror `messenger.capnp` draft 0.4 (branch `msg-guard` of `fs-interfaces`, not tagged); no generated types are used.

## State

Version 0.2.0, not tagged: a new crate, with new methods on the core's `EngineGuard` and `SenderLookup` (core was 0.1.1 before).

| Area | State |
|---|---|
| `Gate::check` (`Allow`, `Ask`, `Refuse`), `revise`, `finished`, `admit_message`, as a pure state machine over a repository | done |
| Risk table per capability, one entry per `Conversation.act` member; what is not in it is `unknown`; the guard and `toolcall.decide` are refused for an AI outright | done, **provisional form** |
| Invitation by a signed `invite`; releases start empty; who reads as an AI by the rule below | done, **rule provisional** |
| `release` per participant, class **and program**, naming where the text goes; `revoke` (all, or of one program); the brake, also against a tampered record | done, **PROVISIONAL**, see "Nothing without a release" |
| Nothing without a release (ADR 0054): without an explicit release a call is `Refuse(NotReleased)`, never `Ask` | done, **PROVISIONAL** reading for `destroys`/`actsForPerson`/`unknown` below |
| `readsSensitive`: a read of a marked object (or one whose mark is unknown) asks every time, cannot be released; mirrored locally, **contract value pending, mirrored locally** | done for `Gate::check_object`; the real source of marks is **not built** (`NoMarks` answers `Unknown`) |
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
| Review of 2026-11-07, fourth: (1) a message that arrives with a tool call in it (`ingest`) went past the guard and was stored `working` after a stop; now every way of storing an agent's call (ingest and report) goes through `repo/report_landing.rs` in the core, which asks the guard before the write and again after it (`tests/stop_late_ingest.rs`, also with a stop between question and write); (2) the retry task's flag is cleared by `Drop`, so a task lost with its runtime no longer keeps a new one from starting (`tests/stop_retry_runtime.rs`); (3) a cancelling write that fails is tried 3 times (50 ms apart) and then reported as an error (core `tests/report_write_fails.rs`). Each reproduced by a test that failed first | done |
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
| Foreign Matrix programs (ADR 0054, point 4c) | nothing: in a Matrix room with programs we do not control (Element) someone can invite an AI and we cannot prevent it. Meant: warn, and offer to leave the room. Needs the connector and a frontend |
| The real source of marks (ADR 0054, "Lesen ist nicht harmlos", ADR 0029) | `NoMarks` answers `Unknown` for every object, which counts as marked: every read through `check_object` asks. The source will be `envelope.sensitive` of `fs-store`, set by the program of the object or by the person, and is set once with `GuardEngine::set_sensitivity` |
| A tool layer that names the object it reads | `Gate::check_object` exists; `Gate::check` names no object and is not looked up (open point 10). No tool layer calls either yet |
| Rights as tools (ADR 0054, "Ohne Befugnis nichts") | no tool at all for delegations, roles, memberships, invitations, store links, hub login; the table only says `NeverForAi` for some |
| Who is calling, proven | `Caller` is built by the tool layer that does not exist yet; ADR 0054 suggests the system user over a Unix socket |

### Tests

Tests for "nothing without a release" and "reading is not harmless" (`release_denies`, `release_revoke`, `release_who`, `sensitive_reads`, `secrets_and_reports`, `no_release_reads`) were written against the trait shells first and failed (27 of them) before the implementation; they run against the real engine. The only double is a source of marks (`Fixed`, `Hangs`, `Panics`), which is a strategy the engine is meant to be given. Existing tests: `World::working()` is `with_ai()` plus a `changes` release in `message`, so that what always asks can still be asked; `World::ask` releases the least in a tool's own program when needed. Against real, encrypted `TursoStore`s in temp files, restarted by dropping every handle and opening the files again. A hand-held clock moves expiry. Test doubles exist only to add a failure or a behaviour around the real thing: a guard repository whose reads and writes fail, wait, hang or panic, over the real one (`Faulty`); providers that hang, panic or cannot stop; a guard that does nothing, to reach the port alone; a device verifier and a stop permission that hang or panic (`stop_judge_hangs`, `stop_verifier_panics`); and, in the core, which cannot depend on this crate, small `EngineGuard` stand-ins that are open once and then shut, and a `Store` that lets writes through and then fails some (`report_write_fails.rs`). The races of the stop are tested against the real guard (`stop_late_ingest.rs`, `stop_late_report.rs`) and a hand-held pause in `Faulty`. Each guarantee was checked by removing it from a clone and seeing a test fail.

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

Only patterns of coding.md section 2 as of 2026-11-07 (commit 7c658d2: it now also has Specification, Value object, Null object and Memento). `none` only where section 2 allows it (error lists, plain data, constants, a single pure function or a small set of them for one concept, thin binary).

| Module | Pattern | Trait / form | Why |
|---|---|---|---|
| `api.rs` | Strategy | traits `Gate`, `Guard` | a tool layer holds `dyn Gate` and has no `Guard` method |
| `state/stage.rs`, `state/calls.rs`, `state/decide.rs`, `state/admin.rs`, `state/doc.rs` | State | enums `CallStage`, `GuardMode` and pure transitions | all rules on a copy; the engine writes the whole result or nothing |
| `risk.rs` | Specification | `can_be_released`, `offers_allow_class`: pure predicates over a risk class, one built from the other | the brake is one place that nothing else may contradict |
| `state/proof.rs` | Specification | `signature_holds`, `check_proof`: pure validation rules over the state and a proof; they answer with a reason, and for a good proof the single-use key | changes nothing, so nothing is burnt by a forged proof |
| `engine/marker.rs` | Memento | the file is the snapshot of a stop (`StopMark`); the functions are its caretaker | the latch is restored after a restart without the store |
| `enrol.rs` | Capability object | `LocalEnrolment`: key administration (verifier, save, log in a fixed order) behind a few ways in | only `GuardEngine::open` hands it out, and holding it is the right to enrol keys (section 2: "a local enrolment") |
| `identity.rs`, `model.rs`, `error.rs`, `lib.rs` | none, allowed by section 2 | one pure function, plain data, error lists, declarations | allowed by section 2 |
| `log.rs` | Value object | `GuardLogEntry` with small constructors (`new`, `about`), `kind` (event name to AS2 kind), `subject` | a small data type for one event |
| `digest.rs`, `signed.rs` | none, allowed by section 2 | a small set of pure functions for one concept each; `Method` enum | section 2: "a small set of them for one concept" |
| `repo/traits.rs`, `repo/turso.rs` | Repository, Adapter | trait `GuardRepository`; `TursoGuardRepository` over `TursoStore` | one opaque record, one log; whatever comes back is checked |
| `engine/*` (`core`, `gate`, `decide`, `questions`, `stop*`, `admin`) | Facade | `GuardEngine`, its `impl` blocks split by concept | fixed order of each operation, which is the guarantee; no trait with provided methods, so no Template method |
| `engine/store_access.rs` | Facade, RAII / Guard | time limits and fail-shut around `GuardRepository`; `RetryRunning` clears the retry flag on `Drop` | not a Decorator: the trait is not implemented again; the flag must not outlive a task lost with its runtime |
| `engine/wiring.rs` | Adapter | `Route` implements the core's `EngineGuard` and `SenderLookup` over the engine | the core knows the traits, not this crate; no cycle |
| `table.rs` | Registry | `TABLE`: capability to rule | becomes schema annotations |
| `program.rs` | none, allowed by section 2 | `program_of`: one pure function | the program of a tool is the part of its name before the first dot |
| `state/releases.rs` | Specification | `is_released`, `has_release_in`, `destinations`: pure predicates over the grants | a grant counts only for participant, class and program together, with a program and words for where the text goes |
| `sensitivity.rs` | Strategy, Value object | trait `SensitivityLookup` (`async fn sensitivity(&self, &ObjectRef) -> Sensitivity`); `ObjectRef` valid on construction | the source of marks changes (later `envelope.sensitive` of `fs-store`); `Unknown`, failure, timeout and panic all read as marked |
| `no_marks.rs` | Null object | `NoMarks`: implements `SensitivityLookup`, learns nothing, answers `Unknown` on purpose | the first source; in doubt the read asks |
| `engine/sensitive.rs` | Facade over Strategy | `read_asks`: the source runs in a task of its own, within `store_wait` | one fixed way to ask it |
| `arguments.rs` | Newtype | `Arguments` | exists only canonical, unique and within the limit |
| `device.rs`, `permission.rs`, `clock.rs` | Strategy | `DeviceVerifier`, `StopPermission`, `Clock` | key type, who may pull it, and time can change |
| core: `guard_hook.rs` | Strategy | `EngineGuard`, `SenderLookup` | who is an AI is the strategy `SenderLookup`; `open_for_reports` is how the core learns that the program is shut |
| core: `repo/guard_port.rs` | Capability object | `GuardPort` | one port, handed out once by `install_guard`; holding it is the right to write what the engine says (section 2: "the port of a guard") |
| core: `repo/guard_turns.rs` | Facade | providers asked in tasks of their own | one fixed order |
| core: `repo/guard_note.rs`, `repo/tool_calls.rs` | Adapter | engine call to `Note`; agent report to `ToolCall` | a report can claim nothing |
| core: `repo/report_landing.rs` | Facade, doubtful (and Strategy through `EngineGuard::open_for_reports`) | `land_reports`: ask, apply, write, ask again | one place for every way of storing an agent's call; shut guard means `cancelled` |
| core: `provider/turn.rs`, `model/tool_call.rs`, `repo/capabilities.rs`, `repo/consts.rs`, `error.rs` | none, allowed by section 2 | plain data; one function filling a report; constants; error list | allowed by section 2 |
| core: `repo/message_index.rs` | none, allowed by section 2 | two functions for one concept (the index fields) | section 2: "a small set of them for one concept" |

## Decisions

**Signed bytes (fixed).** Every field is a 4-byte big-endian length and its bytes; the arguments have a 4-byte count in front:

```
field("FreeSynergy.Messenger guard device proof v1") field(engine identity) field(method) field(signer)
count(arguments) field(argument)... field(nonce)
```

`method` is `invite`, `release`, `revoke`, `uninvite`, `stop`, `reopen`, `decide`, `addDevice`, `removeDevice`. Arguments: `invite`, `uninvite`: profile. `release`: profile, risk by its contract name, program, goesTo (in this order; all four, each as the field above; the program as the caller wrote it, the engine stores it in the one spelling `id_key`; goesTo is free text the person wrote). `revoke`: profile, program (empty field: every program). `stop`, `reopen`: none. `decide`: message id, tool call id, option id, digest, revision as 8 big-endian bytes, nonce empty: `ToolCallDecision` has no nonce. `addDevice`, `removeDevice`: person, key in lowercase hex. The signer is a field beyond what the contract lists, so that a key shared by two persons cannot be replayed between them. The engine identity is 32 random bytes in the state: `GuardEngine::engine_identity`.

**Nothing without a release (PROVISIONAL, reported to the strategy).** ADR 0054 (2026-11-07): an invited participant may do nothing until a person gives a release; the engine refuses (`Verdict::Refuse(Refusal::NotReleased)`), it does not ask, so an AI cannot flood a person with questions. A release is `AiGrant { profile, risk, program, goes_to }`; it holds for that participant, that class and that program together, and for nothing else. The program of a tool is the part of its name before the first dot (`message.open` is `message`; no dot: the whole name; `program_of`, compared in the one spelling of `id_key`). `goes_to` is free text the person wrote, signed by their device, shown in the `summary` of every question of that program; the engine takes it from no AI. A grant with an empty program or empty `goes_to`, as an older record has it, counts for nothing. A new release of the same participant, class and program replaces the old one. Order in `evaluate` after stopped, name, `NeverForAi`, invitation: an `allowOnce` that was approved for exactly these arguments is used; a release of the class in the program allows; otherwise `readsOnly` and `changes` are refused; **interpretation (PROVISIONAL):** what always asks (`destroys`, `actsForPerson`, `unknown` and a marked read) is refused too when the participant has no release of any class in this program, and asks per call when it has one. Consequence: a question never offers `allow-class` any more (only a releasable class could have it, and a releasable class is not asked); a person releases through `Guard::release` on their own device. `decide` still can grant on an old record, with the program of the call and its `goes_to`, which is empty then and so counts for nothing. A stop and a reopen empty the releases as before.

**Reading is not harmless (ADR 0054; contract value pending, mirrored locally).** `readsSensitive` is not in the contract `messages` yet. It is mirrored here: a call record has `sensitive`, and such a call shows as risk `unknown` (which asks every time and offers no release). The engine sets it, from `SensitivityLookup` for the object that `Gate::check_object` names, when the tool's risk is `readsOnly`: `Marked` and `Unknown` set it, `Unmarked` does not; no answer within `store_wait`, a panic, or an error all read as marked. A marked read is never covered by a release of `readsOnly` (no `allowClass`, no "always", `can_be_released` stays false for it); it asks every time, and needs a release of any class in its program first, else it is refused. The mark is set only by the program of the object or the person, through the source; the call's arguments are never read for it in either direction, and an AI cannot remove it. No answer of the person is no access: a question expires (10 minutes) and a late click is refused; the next read asks anew. Secrets never reach an AI: the gate knows that only for the exact name `account.secret` (`NeverForAi`, in any spelling of case and space); see open point 11. Telling something outward is `actsForPerson` in the table (`message.send`, `reply`, `schedule`, `forward`, `react`, `edit`, `conversation.markRead`, `rule.write`, `account.add`) and asks every time; a name that is not in the table is `unknown`, which asks too.

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

**Who is an AI (provisional).** An AI is a participant, not a kind of account. Whether a sender is one is asked through `SenderLookup::sender_kind`: `ai` only for a profile the person invited, else `unknown`; on no answer in time, an unreadable state, or no guard, also `unknown`. Never `person`, and a sender string gives no right: rights go by `Caller`. What counts is the person's invitation list, never what a provider or the AI says of itself. Every message is stored, whatever the sender, also while stopped: a stored message is no action, and mail is not lost. A forged sender can claim `ai` or stay `unknown`, and gets nothing from either. KI = Teilnehmer; the source of the mark will later be `fs-people` (`kind` application); for now provisionally the guard's invited list. Also provisional: the profile is compared with the transport's sender string, where the contract speaks of card ids. A report of a tool call gets no right, whoever sends it. Since the fourth review the same holds for a call that comes in with a message: `report_landing.rs` is the one place that stores an agent's call, and `ingest` and `record_tool_call` both use it. While the program is not open (stopped, damaged, or the guard does not answer), a report whose message was sent by an `ai` or `unknown` sender is stored as `settled`, `cancelled`: `EngineGuard::open_for_reports` asks the engine, and the core asks it under its write lock, so a stop either settles the report with the rest or has shut the guard before it lands. The core waits for that answer up to 1 second under its write lock; the cancelling write after the second look is tried 3 times, 50 ms apart; if all fail the caller gets the error and the store still shows what was written first (`working`): a limit. The guard is shut then, so nothing acts on it and a later stop settles it; the guard never calls the core back from it, so there is no deadlock, and no answer reads as shut. The settling of a stop reads the index before it takes the write lock, so it can miss a message whose first report is being written at that moment; therefore the core asks the guard a second time after the write and cancels the report if the guard has shut meanwhile (a stop's latch is up before its settling begins: either the settling saw the message, or the second look sees the latch). `tests/report_after_stop.rs` in the core. `person` and `software` senders are not affected, and nothing is asked when no guard is installed. Before the stop, and after a `reopen`, a report is `working` as before (`tests/stop_late_report.rs`; core: `tests/report_after_stop.rs`).

**Retries (provisional numbers).** A bare `input_required` can be retried at once. Per question: not sooner than 2 seconds (`RetryTooSoon`), at most 60 retries (`RetriesExhausted`). Both refuse without writing, and the question stays open for the person; an approved call always goes through. Per participant: at most 16 open questions (`TooManyQuestions`), and the same arguments reuse the same question. The counter is in memory, so a restart starts it afresh. A question lives 10 minutes, an unused `allowOnce` 2 minutes. Arguments over 64 KiB are refused: what is shown is never cut off, so what cannot be shown whole cannot be asked about. Characters that hide or reorder text are shown escaped, and refused in names.

**Who may do what.** `invite`: any enrolled person, and that person is who gets asked. `release`, `revoke`, `revoke_program`, `uninvite`: who invited it, or the owner. A release names program and destination; `release` is refused for an empty program (`Empty("program")`), an empty `goesTo` (`Empty("goesTo")`), or a program with a dot or hidden text (`BadName`). `stop`, `reopen`: `StopPermission`; `OwnerOnly` takes the person who owns the program, from `GuardConfig::owner`. Pulling is ADR 0054's permission setting. A signer marked as AI is refused before any key is looked at, and before `StopPermission`, so no implementation can let one through (ADR 0056). `invite` and `release` are refused while stopped. A stop on a state that cannot be read has no key to check against: it needs no proof there, as it can only end work.

**First key (provisional).** `GuardEngine::open` hands back `LocalEnrolment` and nothing else can make one. The program keeps it with its local console. Whoever holds it at the first start puts in the owner's key: trust on first use, on the local machine. After that a key needs a proof by a key of the owner. An invited AI gets no key, and a person with a key cannot be invited. `recover_damaged` only works on a state that was read and is not one; never because the store did not answer.

**A stop beats a reopen that began earlier.** `reopen` takes a stop count when it is asked for. Latch, mark and marker go only if no stop came in since, checked and cleared in one step under the mark's lock; otherwise nothing is cleared, the marker is put back, and `reopen` answers `Stopped`. A stop that waits for the lock never gives up silently: the retry task writes marker and store as soon as the lock is free. The latch is never down between a stop's arrival and its write.

**Order of a stop.** In the code (`stop.rs`): `pending` goes up at once; then the proof is judged, up to `judge_wait`; only then the mark is set (`stopped`), and then marker and store, what is shown, providers and log. This is safe because `is_latched` counts `pending` as well: from the first line of the stop `check` refuses, and a stop whose proof is bad takes `pending` back down without having set `stopped`. `check` sees the latch without a lock. Providers run in tasks of their own with 5 seconds together; one that hangs or panics reports `NoAnswer`. The latch is not set again under the lock any more (since 86adc2b): it goes up once, at the start (`stopped.store(true)` only in `store_access.rs`, `mark_unsaved`), and a `reopen` clears it only through `clear_stop_unless_newer`, which compares the stop count and clears in one step under the mark's lock. A `reopen` that began first therefore cannot swallow the stop. The judging of the proof runs in a task of its own, within `judge_wait`: a verifier or permission that panics there, or does not answer in time, makes it a stop by nobody, with marker and retry as for any stop (`tests/stop_verifier_panics.rs`). Before 2026-11-07 such a panic left the latch up in memory only, so a restart lost the stop when the store still said "open".

**Own store.** Give the guard a store of its own, as `UnloggedObjects` advises. The rules also hold on a store shared with the messenger, and that worst case is what `tests/out_of_reach.rs` tries, but `fs-hub` pushes a store's log, so a shared store would send profile and tool names along. The repository refuses a store that is not encrypted.

## What cannot be enforced within one process

Plainly: **Rust privacy is no boundary against code in the same process.** A provider or plugin running in this process can read memory, open the store file again, or simply act without asking. Whoever can write the store can set the mode to `open`, add a key, or clear used proofs, and nothing here can detect it, because any key to detect it with would lie beside the record. What is built holds against every path through the `Messenger`, `Ingest` and `Provider` traits, not against code that leaves them.

That is why ADR 0056 requires **agents in their own sandbox under their own system user**: the store file and its key are out of reach of the agent because the operating system says so, not this crate. Connectors that run in the engine must be reviewed. The stop also only reaches what runs through our programs (ADR 0054): an AI with its own shell is outside it, and a provider that cannot end a turn is reported, not ended.

Also: `Verdict::Allow` is a snapshot. What has started is not called back, except through the provider's `stop_turns`. A write that fails during a stop is lost if the program dies before a later write works, and the report says so. The clock comes from the system: set back, it keeps a question alive for longer.

## Open points

0. **Patterns against coding.md section 2 (closed except these doubts, to the strategy).** Checked on 2026-11-07 against the section with Specification, Value object, Memento, Null object and Capability object. `risk.rs`, `state/proof.rs` Specification (`check_proof` answers with a reason and a key rather than a bool: a stretch of "predicate"). `engine/marker.rs` Memento (whether a readable JSON file is "opaque" is a stretch). `enrol.rs` and core `guard_port.rs` Capability object. `log.rs` Value object (the fields are public and mutable, so "immutable" is a stretch). `digest.rs`, `signed.rs`, core `message_index.rs`: `none`. Doubtful, left as the nearest: core `repo/report_landing.rs` as Facade (`land_reports` is a method on `Inner`, not a type of its own), `engine/*` as Facade (one `GuardEngine` split into files; `signed_change` in `admin.rs` is a helper function with a closure), `state/doc.rs` as State (a record with a mode), `table.rs` as Registry (looked up by capability id, but the entry is a rule as data, not behaviour).

1. The table's form, and its classes, are a proposal for the owner: `account.add` and `rule.write` as `actsForPerson` are my reading.
2. `CONTRACT_VERSION` stays `0.3` until draft 0.4 is tagged, though `ToolCallDecision.revision` and the problem types already mirror 0.4.
3. A lost last key, and the same question for groups: who may step in without opening a way for an AI?
4. Used proofs are kept for good. A proof with a time in it would allow dropping them; the contract has none.
5. Refused attempts are not logged: a refusal changes nothing, and a caller could otherwise fill the log. A person might want to see them; that needs a limit first.
6. A message with an open call stored before this version has no `open_call` index, so a stop does not settle what it shows. The guard's own record is not affected.
7. `Gate::admit_message` is not called by a tool layer yet; the engine uses the same rule (`admission`) for `EngineGuard::open_for_reports`, where "not invited" counts as open (a report gets no right either way). `admit_message` itself stays for a tool layer; remove it if none wants it.
8. A report on a message whose stored `sender_kind` is `unknown` or `ai` is cancelled while shut, also when the message is an outgoing one of the person's own (`send_record.rs` stores `unknown`). Harmless (a report on it is an AI's work), but it is why the rule goes by the stored kind and not by who reported: the contract's `recordToolCall` does not say who reports.
9. A hanging disk: the marker work runs on the runtime's blocking threads. `tests/reopen_stalled_disk.rs` stalls them all and shows that `reopen` still ends. A `stop` writes the marker with a time limit too, and the retry task goes on; what a disk that never comes back leaves is the stated limit above.
10. **A read that names no object is not looked up (to the strategy).** `Gate::check` has no object, so a released `readsOnly` class still allows `message.open` through it (`tests/no_release_reads.rs`); only `check_object` looks the object up. A flag per tool in the table (this tool reads content of an object, so `check` without an object counts as `Unknown`, which asks) would close it; not done, because it would invent a taxonomy of tools and ask for every read until the real source exists.
11. **Secrets.** The gate knows a tool as secret only by the exact name `account.secret`. Another name for a tool that hands out a key, token or credential (`account.token`, `attachment.key`) is `unknown` to it: refused without a release in its program, asked with one, but not `NeverForAi`. Whether the contract names such tools, or marks them, is for the strategy (`tests/secrets_and_reports.rs` states this).
12. **PROVISIONAL, reported to the strategy:** (a) for `destroys`, `actsForPerson`, `unknown` and a marked read: refused when the participant has no release of any class in the program, asked per call when it has one; (b) the question's `summary` carries the destinations of the program's releases (it was empty); (c) `Refusal::NotReleased` is our word, not a contract word (the contract has `Verdict` only here); (d) `revoke_program` and the extra fields `program` and `goesTo` of `AiGrant` are not in the contract; (e) the timeout of a question is its expiry, not a verdict: `Gate` does not wait, and a tool layer that gives up must not run the call; (f) `allowClass` through a question is gone (see above), so the option stays in `options_for` for old records only; (g) a marked read is shown as risk `unknown` until the contract has `readsSensitive`.
13. Where the text of a release goes is free text; nothing checks that it is true, or that it names the provider. The question shows it so that the person can see it.
