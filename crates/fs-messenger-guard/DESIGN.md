# fs-messenger-guard – Design

The AI guard of FreeSynergy.Messenger: invitation, release, brake, emergency stop (ADR 0054, 0056). In the engine, below every frontend, without network and without a model. When in doubt it stays shut. Names mirror `messenger.capnp` draft 0.4 (branch `msg-guard` of `fs-interfaces`, not tagged); no generated types are used.

## State

| Area | State |
|---|---|
| `Gate::check` (`Allow`, `Ask`, `Refuse`), `revise`, `finished`, `admit_message`, as a pure state machine over a repository | done |
| Risk table per capability, one entry per `Conversation.act` member; what is not in it is `unknown`; the guard and `toolcall.decide` are refused for an AI outright | done, **provisional form** |
| Invitation by a signed `invite`; releases start empty; messages of an AI by the rule below | done, **rule provisional** |
| `release` per participant and class; `revoke`; the brake, also against a tampered record | done |
| `decide`, routed from `Messenger::act`; every refusal listed in the contract, typed by the four `Problem.type` URIs; `toolCall` event on stale and settled | done |
| Device proof: `DeviceVerifier`, Ed25519 by `ed25519-compact`; signed bytes below | done, **key type provisional** |
| Local enrolment, first key by trust on first use | done, **provisional** |
| Emergency stop: latch, persist, settle, providers, log. Read-through fail-shut. `reopen` | done |
| `StopPermission`: `OwnerOnly`. Per group: `Unsupported` | done for one owner |
| Activity log without secrets, out of reach of undo | done |
| `sender_kind`: `SenderLookup` over the guard's invited list | done, **provisional**: `fs-people` pins `fs-store` v0.2.0, this repo v0.3.0, so it cannot be a dependency |
| Ratings, trust levels through third parties, MCP, ACP, RPC, GUI, model providers | **not built**; see below |

### Not built

| What | How it shows |
|---|---|
| A stop per group | `Guard::stop_group` and `StopPermission::may_stop_group` answer `Unsupported("guard.stop.group")`. ADR 0054 point 5 says it is a normal permission setting: if someone gives everyone the right, everyone may. Groups come with `fs-groups` |
| Trust levels 0 to 3, ratings, operators | nothing here reads them: only invitation and release count. ADR 0054 point 6 leaves the basis open |
| A `Messenger.guard` method (contract `guard @8`) | deliberately **not** on the Rust trait, so no path leads there; the RPC layer composes it for a person's device |
| MCP, `input_required`, `requestState`, ACP | a tool layer calls `Gate`; `Verdict::Ask` has what it needs. Nothing untested is claimed |
| Replacing a lost last key while the state is intact | no path: AIs stay shut out, the rest of the messenger works. Open point 3 |
| Who is calling, proven | `Caller` is built by the tool layer that does not exist yet; ADR 0054 suggests the system user over a Unix socket |

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

| Module | Pattern | Why |
|---|---|---|
| `api.rs` | Strategy, interface segregation | a tool layer holds `dyn Gate` and has no `Guard` method |
| `state/*` | State + Specification | all rules as pure functions on a copy; the engine writes the whole result or nothing |
| `repo/traits.rs`, `repo/turso.rs` | Repository, Adapter | one opaque record, one log; whatever comes back is checked |
| `engine/*` | Facade, Template method | fixed order of each operation, which is the guarantee |
| `engine/wiring.rs` | Adapter | the core's hooks over the engine, without a cycle |
| `table.rs` | Registry | capability to rule; becomes schema annotations |
| `risk.rs` | Specification | the brake in one place |
| `arguments.rs` | Newtype as proof | an `Arguments` exists only canonical, unique and within the limit |
| `digest.rs`, `signed.rs` | Strategy, Builder (pure) | one way to bytes |
| `device.rs`, `permission.rs`, `clock.rs` | Strategy | key type, who may pull it, and time can change |
| `enrol.rs` | Capability object | only `open` hands it out |
| `log.rs` | Adapter | a small event to a log-only activity |
| core: `guard_hook.rs` | Strategy | the core knows the traits, not this crate |
| core: `repo/guard_port.rs`, `guard_note.rs`, `guard_turns.rs` | Capability object, Adapter, Template method | one port, once; engine calls; providers in their own tasks |

## Decisions

**Signed bytes (fixed).** Every field is a 4-byte big-endian length and its bytes; the arguments have a 4-byte count in front:

```
field("FreeSynergy.Messenger guard device proof v1") field(engine identity) field(method) field(signer)
count(arguments) field(argument)... field(nonce)
```

`method` is `invite`, `release`, `revoke`, `stop`, `reopen`, `decide`, `addDevice`, `removeDevice`. Arguments: `invite`, `revoke`: profile. `release`: profile, risk by its contract name. `stop`, `reopen`: none. `decide`: message id, tool call id, option id, digest, revision as 8 big-endian bytes, nonce empty: `ToolCallDecision` has no nonce. `addDevice`, `removeDevice`: person, key in lowercase hex. The signer is a field beyond what the contract lists, so that a key shared by two persons cannot be replayed between them. The engine identity is 32 random bytes in the state: `GuardEngine::engine_identity`.

**Digest.** SHA-256 over length-prefixed (8-byte) fields: fixed context `FreeSynergy.Messenger tool call digest v1`, caller, tool name, then `input` and the canonical form, or the explicit `no input`. Canonical form is ours, not RFC 8785: keys sorted by bytes at every level, no white space, values as `serde_json` writes them. `1` and `1.0` differ, which at worst asks again. Keys are sorted by hand, because `serde_json`'s `preserve_order` can be turned on by any crate in a build. A duplicate key is refused: two programs can read it differently.

**Crates.** `ed25519-compact` 2.6 (pure Rust, MIT) with default features off. `ed25519-dalek` and `curve25519-dalek` are BSD-3-Clause, so they do not meet the rule of MIT or Apache-2.0. `sha2` and `getrandom`: MIT OR Apache-2.0.

**Single use.** A `GuardProof` is single use by signer and nonce, across all methods, 16 to 64 bytes. A decision by its signed bytes. Never by the signature's bytes: another valid signature over the same bytes exists, and is tested. Nothing is used up before the signature holds, so a forged proof cannot burn a nonce. Stored in the state, for good.

**One document, unlogged.** Mode, keys, invitations, releases, calls and used proofs are one object, written with `UnloggedObjects`, so a decision, its used proof and its release are atomic, and there is no before image that undo could restore. Every operation reads it through, so a record gone bad is seen at once. Arguments are not stored, only the digest. Cost, not measured: up to one JSON parse per `check`, and a used proof adds about 70 bytes for good.

**What is shown decides nothing.** A `ToolCall` in a message is a display of the guard's record. A question has no agent reference and no alias, so no report or delivery can name it. Undo can bring back what a message shows; a click on it is refused by the record, and the `toolCall` event of that refusal says how the call really stands. Engine writes use the actor `urn:freesynergy:messenger:guard`, so a person's own undo does not even reach them.

**Refused decisions.** `Error::DecisionRefused` carries `DecisionProblem`, with `type_uri()` as in the contract: `decisionStale` (digest or revision no longer match; also a call already answered and not over, since what was shown has moved on), `decisionSettled`, `decisionNotAsked`, `decisionInvalid` (everything else, also an id that is no call at this place, a stopped program with a call still open, and a damaged state). A stopped program answers `decisionSettled` for its cancelled calls, as that is what a card needs. On stale and settled a `toolCall` event follows, from the guard's record, over an in-memory channel beside the store feed: nothing is written, not even to the log. It goes out before the signature is checked, and says nothing that the subscription does not already show. A listener more than 64 announcements behind misses it and reads the message. Failures to write are `Error::GuardRefused`, not a decision problem.

**The rule for messages of an AI (provisional).** An account of kind `agent` carries AI participants only; the kind is the person's setting, not what a provider says of itself. From such an account: invited and open is stored and marked `ai`; uninvited or stopped is refused (`Error::GuardRefused`), stored nowhere, and a sync counts it as `rejected`; without a guard too. From any other account nothing can tell an AI from a person: stored as any message, `unknown` unless the sender string is exactly an invited profile, then `ai`, also while stopped: a stored message is no action, and mail is not lost. No sender ever reads `person` here, and a sender string gives no right: rights go by `Caller`. A forged sender can claim `ai` or stay `unknown`. Provisional also: the profile is compared with the transport's sender string, where the contract speaks of card ids.

**Retries (provisional numbers).** A bare `input_required` can be retried at once. Per question: not sooner than 2 seconds (`RetryTooSoon`), at most 60 retries (`RetriesExhausted`). Both refuse without writing, and the question stays open for the person; an approved call always goes through. Per participant: at most 16 open questions (`TooManyQuestions`), and the same arguments reuse the same question. The counter is in memory, so a restart starts it afresh. A question lives 10 minutes, an unused `allowOnce` 2 minutes. Arguments over 64 KiB are refused: what is shown is never cut off, so what cannot be shown whole cannot be asked about. Characters that hide or reorder text are shown escaped, and refused in names.

**Who may do what.** `invite`: any enrolled person, and that person is who gets asked. `release`, `revoke`: who invited it, or the owner. `stop`, `reopen`: `StopPermission`; `OwnerOnly` takes the person who owns the program, from `GuardConfig::owner`. Pulling is ADR 0054's permission setting. A signer marked as AI is refused before any key is looked at, and before `StopPermission`, so no implementation can let one through (ADR 0056). `invite` and `release` are refused while stopped. A stop on a state that cannot be read has no key to check against: it needs no proof there, as it can only end work.

**First key (provisional).** `GuardEngine::open` hands back `LocalEnrolment` and nothing else can make one. The program keeps it with its local console. Whoever holds it at the first start puts in the owner's key: trust on first use, on the local machine. After that a key needs a proof by a key of the owner. An invited AI gets no key, and a person with a key cannot be invited. `recover_damaged` only works on a state that was read and is not one; never because the store did not answer.

**Order of a stop.** Who pulls it, latch, write, settle what is shown, providers, log. `check` sees the latch without a lock. Providers run in tasks of their own with 5 seconds together; one that hangs or panics reports `NoAnswer`. The latch is set again under the lock, so a `reopen` that began first cannot swallow the stop.

**Own store.** Give the guard a store of its own, as `UnloggedObjects` advises. The rules also hold on a store shared with the messenger, and that worst case is what `tests/out_of_reach.rs` tries, but `fs-hub` pushes a store's log, so a shared store would send profile and tool names along. The repository refuses a store that is not encrypted.

## What cannot be enforced within one process

Plainly: **Rust privacy is no boundary against code in the same process.** A provider or plugin running in this process can read memory, open the store file again, or simply act without asking. Whoever can write the store can set the mode to `open`, add a key, or clear used proofs, and nothing here can detect it, because any key to detect it with would lie beside the record. What is built holds against every path through the `Messenger`, `Ingest` and `Provider` traits, not against code that leaves them.

That is why ADR 0056 requires **agents in their own sandbox under their own system user**: the store file and its key are out of reach of the agent because the operating system says so, not this crate. Connectors that run in the engine must be reviewed. The stop also only reaches what runs through our programs (ADR 0054): an AI with its own shell is outside it, and a provider that cannot end a turn is reported, not ended.

Also: `Verdict::Allow` is a snapshot. What has started is not called back, except through the provider's `stop_turns`. A write that fails during a stop is lost if the program dies before a later write works, and the report says so. The clock comes from the system: set back, it keeps a question alive for longer.

## Open points

1. The table's form, and its classes, are a proposal for the owner: `account.add` and `rule.write` as `actsForPerson` are my reading.
2. `CONTRACT_VERSION` stays `0.3` until draft 0.4 is tagged, though `ToolCallDecision.revision` and the problem types already mirror 0.4.
3. A lost last key, and the same question for groups: who may step in without opening a way for an AI?
4. Used proofs are kept for good. A proof with a time in it would allow dropping them; the contract has none.
5. Refused attempts are not logged: a refusal changes nothing, and a caller could otherwise fill the log. A person might want to see them; that needs a limit first.
6. A message with an open call stored before this version has no `open_call` index, so a stop does not settle what it shows. The guard's own record is not affected.
7. A provider reporting a new call after a stop still shows it as `working`: shown, and it gets no right.
