# fs-messenger-ai – Design

**Not checked against a real server.** llama.cpp stayed off and localhost:8080 was not touched. There is no API key and api.anthropic.com was never called. Every statement below about a wire format comes from documentation listed under "Sources", or from a scripted local server that stands in for the wire.

The AI model provider of FreeSynergy.Messenger: one model behind the core's `Provider` trait, in two wire formats, llama.cpp and the Anthropic Messages API. No GUI, no RPC. Nothing here runs a tool. There is no MCP yet, and no ACP: ADR 0056 needs a sandbox for that, and it is not in this step.

## State

Version 0.1.1, not tagged. **As things stand, no text reaches a model.** The core refuses every honest plan, see "Encrypted only meets a model". That is the first thing to decide. Everything else below is built and tested behind that gate, by a stand-in that is labelled as one.

| Area | State |
|---|---|
| `ModelProvider`, `Provider` for llama.cpp `POST /v1/chat/completions` and Anthropic `POST /v1/messages`, streaming | done, scripted server only |
| Plan: `none` on a loopback address without TLS, `transport-only` with TLS, the model named as missing a key, never end to end. The core refuses both | done, **owner's decision open** |
| Receipt says what is true, also if a plan claimed more: the core then answers `EncryptionDowngraded` | done |
| Invitation: only the invited profile as recipient, the guard asked in `send` and again before the first byte. Nothing on the wire otherwise. An account must be of kind `agent` | done |
| Answer as a message through `Ingest::ingest`: `sender_kind` from the guard's lookup, `text/markdown`, reasoning apart, reply to the person's message, same conversation | done |
| Live side: `created`, `text`, `state` as the core's own `Event` values on this crate's channel. Offsets in UTF-8 bytes per part, whole code points; `LiveTurn` has the text so far | done, **not on the messenger's subscription**; "Wanted" 2 |
| Stored state while an answer is written, and of a failed answer | **not possible yet**: stored once at the end, as `delivered`, a failed one says so in its name; "Wanted" 2, 3 |
| `failed` with the text kept: breaks off, repeats, malformed, invalid UTF-8, wrong content type, never ends, too large, an error event, a stop | done |
| `stop_turns`: closes the connection, also through `Messenger::stop` and the emergency stop; answers only after the close | done |
| Tool requests shown through `record_tool_call`, settled at once as `refused` or `failed`. Nothing runs | done |
| A second turn in a conversation hears the turns that ended well, in memory | done, lost on restart |
| Usage: tokens per turn, added to the conversation's total. Context use as each API allows. Cost not at all: neither API states one | done |
| Secrets: `ApiKey`, redacted in `Debug` and `Display`, wiped on drop, sensitive header; an echo in text, reasoning, tool names and arguments is taken out, also split over deltas | done, literal matches only |
| Limits: bytes, one event, idle, whole turn, retries, `retry-after` cap, request size, reasoning, tool calls | done, numbers provisional |
| TLS required except on a literal loopback address; no redirects, no proxy, one origin | done |
| Remote: `RemoteChoice`, per exact text, once, runs out after 2 minutes; `data_path` for a frontend to show | done, behind the gate |

### Not built

| What | How it shows |
|---|---|
| ACP, MCP, tool execution, ratings, GUI, RPC | not in this step. `ToolOffer` only tells a model what it may ask for |
| `Provider::fetch` | `message.fetch` reported unsupported: answers come in as a turn ends |
| Files to a model | a draft with an attachment is refused, not sent without it |
| Sending a tool result back | nothing runs, so nothing answers; the model hears nothing of its request in a later turn |
| Keeping the wire bytes as `original` (ADR 0030) | `RawMessage.original` stays empty: a stream has no one original, and it could hold an echo of the key. For the owner to judge |
| Continuing after `pause_turn`, `fallbacks`, prompt caching, effort | not sent; a `refusal` or `pause_turn` ends the turn as `failed`, named |
| A limit across conversations | one turn per conversation only. llama.cpp's parallel slots are the operator's matter |

## Encrypted only meets a model (ADR 0018)

A model has to read the text, so end-to-end encryption to a model cannot exist. `encryption_plan` therefore answers what is true of the way there, and names the model in `missing_keys`:

| Way | `DataPath` | Plan | Why |
|---|---|---|---|
| `http://127.0.0.1:...`, `http://[::1]:...` | `ThisDevice` | `none` | Nothing protects it, and it does not leave the device. The contract's `Encryption` has no value for that, and `none` is the nearest that claims nothing false |
| `https://` to a literal loopback address | `ThisDevice` | `transport-only` | |
| `https://anything-else`, also `https://localhost` | `LeavesDevice` | `transport-only` | It leaves over TLS, and the operator of the server reads it. A name counts as remote: a resolver decides where it goes |

**What happens:** the core refuses both, `Error::EncryptionRefused`, before anything is written or sent; what the person wrote stays a draft. `tests/plan.rs` shows it for loopback and for remote, with zero connections on the scripted server. This goes further than expected: **loopback is refused too**, because the gate only passes `mls`, `openpgp`, `megolm`, and claiming one of those would be a lie. `Provider::send` is therefore unreachable in a real program today. Neither the core nor the plan was bent to change that, and there is deliberately no side entry in this crate that sends without the core's gate.

**Decision for the owner.** The messenger cannot talk to a model until one of these is chosen:

1. **Leave it.** No model gets text. This crate waits.
2. **Local only:** a new contract value for "stays on this device" that the gate passes. It costs a jump of the contract, and the core would be believing a provider's word for it (only a loopback `Endpoint` can make that claim here, and connectors are reviewed anyway).
3. **Also remote:** a separate, visible way past the gate, for a person who chose this model, as ADR 0018 already does for bridges ("sichtbar als nicht Ende-zu-Ende"). The message stays marked `transport-only`. `RemoteChoice` is the shape this would take on the provider's side.

My reading, as a proposal only: 2 and 3 together fit the assistant page ("local by default; sending to a remote model is always the person's choice, and visible"). ADR 0018 was written about people writing to people, and a model that must read the text is the recipient, not someone on the way.

**How the tests get past the gate:** `PastTheGate` in `tests/common/mod.rs` lies in its plan, and exists nowhere else. It stands in for the decision. Everything behind it is the real provider, and `Draft.send` still ends in `EncryptionDowngraded`, because the receipt tells the truth. A real program has no such wrapper.

**Sending to a remote model is the person's choice:** even past the gate, a `LeavesDevice` provider sends only with a `RemoteChoice` for exactly this text. A frontend builds that from a person's action after showing `data_path`. Without it the send fails and the message reads `failed`. A message saying "I agree" is text; `tests/remote.rs`.

**Hostile message bodies are data:** a body is one JSON string in a `user` message. `system` comes from the program's configuration only and never holds a message. No field of a request is built from a body, and nothing a body or a model writes reaches `RemoteChoice`, a `Gate`, a `Guard`, or an engine field of a tool call.

## Sources

Read on 2026-11-06, between 19:51 and 20:00 UTC. Copies are in `.scratch/t2-ai/docs`, not in the repository.

### Anthropic Messages API

The `claude-api` skill first (bundled 2.1.316: `curl/examples.md`, `shared/error-codes.md`), then the platform pages below. **They differ on one point: the skill's cURL examples use `x-api-key`; the API overview calls that "Legacy fallback for `Authorization`, still supported". This crate follows the platform page.**

| Wire detail | Source (`platform.claude.com/docs/en/...`), quoted |
|---|---|
| `Authorization: Bearer <key>` | `api/overview`: "`Authorization` ... `Bearer <token>`, where `<token>` is your API key" |
| `anthropic-version: 2026-10-21` | `api/versioning`; `api/overview` |
| `temperature`, `top_p`, `top_k` not sent | `api/versioning`: "A request that sends any of them returns a 400 error on every model" |
| Body: `model`, `max_tokens`, `messages`, `stream`, `system`, `tools` with `input_schema` | `api/messages`, `build-with-claude/streaming` |
| `thinking: {type: "adaptive", display: "summarized"}`, only if `ask_reasoning` | skill, thinking section; "`\"omitted\"` (the default)" means no reasoning text otherwise |
| Event flow; `message_start`, `content_block_start`, `_delta`, `_stop`, `message_delta`, `message_stop` | `build-with-claude/streaming`, "Event types" |
| `ping`; unknown types ignored | the same: "your code should handle unknown event types gracefully" |
| `text_delta`, `thinking_delta`, `signature_delta`, `input_json_delta` | the same, "Content block delta types" |
| Usage cumulative in `message_delta` | the same: "The token counts shown in the `usage` field of the `message_delta` event are *cumulative*" |
| Input total | `api/messages`: "Total input tokens in a request is the summation of `input_tokens`, `cache_creation_input_tokens`, and `cache_read_input_tokens`" |
| `stop_reason` values | `api/messages` |
| Error event, `error.type` | `build-with-claude/streaming`, "Error events"; `api/errors` |
| Retries on 429, 500, 529; `retry-after` in seconds, not on every 429 | `api/errors`; `api/rate-limits`: "The number of seconds to wait until you can retry the request" |
| No cost in the API's answer | `api/messages`, the `usage` fields: tokens only |

### llama.cpp

`tools/server/README.md`, `ggml-org/llama.cpp`, branch `master`, last changed by d02b4c0d66 (2026-11-02). **The form used is the OpenAI-compatible `POST /v1/chat/completions`.** The task asked for whichever is documented as stable: **the README documents none as stable.** It says of this one "While no strong claims of compatibility with OpenAI API spec is being made, in our experience it suffices to support many apps". It is chosen because the native `/completion` takes a finished `prompt`, so the client would build roles into text itself, which is how a body turns into an instruction. Here the server's chat template keeps them apart.

| Wire detail | Source, quoted |
|---|---|
| `Authorization: Bearer`, only if the server has `--api-key` | README: `--api-key KEY`; examples: `-H "Authorization: Bearer no-key"` |
| `messages` in roles, `model`, tools `--jinja` | README, "POST `/v1/chat/completions`" |
| `reasoning_content` | README: "The server supports parsing and returning reasoning via the `reasoning_content` field" |
| `usage.prompt_tokens`, `completion_tokens` | README, "Timings and context usage" |
| Context use | README: "The total number of tokens in context is equal to `prompt_n + cache_n + predicted_n`" |
| Error status body, `error.type` | README, "API errors" |

**The README does not show a streamed chunk.** It says "See OpenAI Chat Completions API documentation". What follows was read in the source and tests at d02b4c0d66, which is not documentation and can change without notice:

| Wire detail | Where |
|---|---|
| `data: [DONE]` | `tools/server/server-context.cpp`: `output = "data: [DONE]\n\n"` |
| A chunk: `id`, `choices[].delta`, `finish_reason` `stop`, `length`, `tool_calls` | `tools/server/server-task.cpp`, `to_json_oaicompat_chat_stream` |
| Usage in a last chunk with empty `choices`; `stream_options.include_usage` | `server-task.cpp`; `tests/unit/test_chat_completion.py` |
| `delta.tool_calls[].index`, `id`, `function.name`, `function.arguments`, in pieces | `tools/server/tests/utils.py` |
| An error in the stream: `data: {"error": ...}` | `server-context.cpp`, `format_error` |

## Pattern map (ADR 0043)

| Module | Pattern | Why |
|---|---|---|
| `provider/facade.rs` | Facade + Strategy | the core sees `Provider`; behind it one flow, one of two dialects |
| `provider/plan.rs` | Specification | what the plan says and what is refused, as pure rules |
| `provider/choice.rs` | Command, used once | what a person did, for one exact text |
| `provider/host.rs` | Strategy (interface segregation) | a provider sees `Messenger`, `Ingest`, `Gate`; held weakly, so it keeps no store's claim |
| `wire/dialect.rs`, `wire/anthropic.rs`, `wire/llama.rs` | Strategy | the two formats, down to small events that claim nothing |
| `wire/anthropic_stream.rs` | State | walks the documented flow and refuses to leave it |
| `wire/sse.rs` | Iterator, fed by chunks | a line is taken only when whole |
| `wire/http.rs` | Facade, Template method | one origin; the attempts of one request |
| `turn/run.rs`, `turn/finish.rs` | Template method | fixed order, each step can only end in `failed` with the text kept |
| `turn/assemble.rs`, `turn/tools.rs` | Builder | offsets, whole code points, caps |
| `turn/live.rs` | Observer | the contract's events, until the core has a way for them |
| `turn/registry.rs` | Registry + RAII guard | one turn per conversation, and the place comes back, also on a panic |
| `turn/history.rs` | Repository, in memory | what a model hears |
| `redact.rs`, `secret.rs`, `endpoint.rs` | Strategy, Newtype | an echo goes; a key cannot be printed; an `Endpoint` exists only if it is allowed |
| `tls.rs` | Strategy | whose certificates count; no "accept invalid" |

## Decisions

* **The answer is stored once, at the end.** The core stores a provider's message whole, as `delivered`, and offers no update of its text. So the live view has an id of its own, `LiveTurn.stored` and the stored message's `job_ref` (`turn:<id>`) link the two, and a crash during a turn loses the text so far. Ruled out: storing first with `undecrypted: true` and replacing it through the core's upgrade path. That would give one id and the text at once, and it would say "could not be decrypted" about something that never was. That is a false encryption state.
* **`send` answers at once, and the turn waits for the core.** The core learns the conversation's alias from the receipt, and a stop needs it. So the turn runs in its own task and sends nothing until the person's message reads other than `sending`, at most 2 seconds. Otherwise it fails as `NotRecorded`.
* **Its place first, then the guard.** The guard sets its latch before it asks providers. A stop that finds no turn registered yet has set the latch, so `admit_message` after registering refuses. A stop that does find it cancels it. Asked again before the first byte.
* **A stop waits for the close, not for the store.** `stop_turns` answers once the response has been dropped, at most 2 seconds, otherwise an error, which the guard reports. Storing comes after, because the guard may hold its own lock.
* **After an emergency stop the answer is not stored.** The guard refuses every AI message then. The text stays in `LiveTurn` only. After `Conversation.stop` the guard is still open, so it is stored and reads "Answer cut off: stopped".
* **Tool calls are settled at once.** `working` would claim something runs, and it would hold `open_call` for ever. `refused` says the request was not carried out; `failed` if the stream broke while writing it. Only the model's name and arguments go in, as agent text, into `name` and `summary`. Reference: our position plus its id, so a repeated id cannot overwrite another call. They are recorded after the answer is stored, since a call needs a message, so a person sees them when the turn ends, not while it runs.
* **Only a known good end is `sent`.** `end_turn`, `stop_sequence`, `tool_use`; `stop`, `tool_calls`. Anything else is `failed`, named, with the text kept: `max_tokens`, `length`, `refusal`, `pause_turn`, one nobody knows yet. Cut off is not finished.
* **A server's text is not shown.** A failure holds a status code, a short snake_case name (`rate_limit_error`), or our own words. Neither an error's `message` nor a header goes anywhere. A short name that holds the key is scrubbed too. This came from a test that found the opposite.
* **A huge `retry-after` ends the turn at once,** naming the wait, instead of sitting it out or asking again early (the docs: "Earlier retries fail"). Without a number, as with a date, the backoff applies. Retries stop as soon as an answer began, and after a connection that did reach the server: a model bills for what it wrote. Only a failed connect is tried again without a status.
* **Invalid UTF-8 ends the turn.** A replacement character would be a guess in somebody's text. A lone surrogate in JSON is malformed, the same way.
* **A failed turn leaves nothing in the history,** so a model is never told it said what it did not finish, and roles keep alternating.
* **A turn that failed before any text still leaves a message,** so the person sees what became of it: empty, from the model's profile, with this crate's words in its name, and not the model's. Only a refusal of the guard leaves none, since that is what its word means.
* **One turn per conversation.** A second send while one runs is refused and reads `failed`.
* **No logging,** so no log can hold a secret or a body. `tracing` is not a dependency. Failures are in `TurnFailure`.
* **`ToolOffer` comes from the program.** Empty by default. Without any, a model has nothing to ask for; a server that sends a call anyway gets it shown.
* **The model's name is configuration.** `ModelConfig::new` refuses an empty one, and only the tests name one.

## Tests

107 tests: 21 unit, 86 integration. Real core, real guard, real encrypted `TursoStore`s in temp files, real device signatures.

* **`tests/common/wire.rs` stands in for the wire only**, and says so: hand-written HTTP/1.1, plain on loopback or real TLS with a certificate of its own. It plays bytes written from the documentation above. It proves how this code handles those bytes, not what a real server sends.
* **`PastTheGate` stands in for the owner's decision**, see above.
* **`RefusesFromTheSecondAsk` in `tests/admission.rs`** adds one behaviour around the real guard, because a stop cannot be timed between two awaits.

**Each guarantee was taken out once, in a throwaway clone, to see whether a test notices (2026-11-06):** 34 cases, such as following redirects, reading the proxy from the environment, no content type check, sitting out a huge wait, no retry limit, not hearing a stop, each limit, lossy UTF-8, not asking the guard in `send` or before the first byte, no remote choice, a plan or receipt claiming end to end, any account, any recipient, dropping files, each path of the key's echo, failed turns in the history, reasoning in the answer, tool calls left working, offsets in characters, believing a cost. The first run left three standing. Two were weak tests and are fixed: a second `message_start` was also caught by another rule, and an early state event went unseen on a single thread. The third was found by a unit test the run had not included. All 34 are caught now. That covers these cases, not every fault.

**The first run of the hostile tests found two real faults in the code:** an error's `type` passed a key that looks like a name into a stored message, and text before a fault in the same chunk was lost. Both fixed; both have tests.

## Limits (known, stated)

* **Rust privacy is no boundary inside one process,** as in the guard's `DESIGN.md`. `RemoteChoice` and `Gate` hold against every path through the traits, not against code in the same process. The provider sees the person's text in memory.
* **An echo is found only as the literal key:** base64, or the key with other text between its parts, is not. What may be the key's start waits for the next piece, so a text ending in such bytes arrives a little later.
* **Not wiped:** the header `reqwest` builds from the key, and what the allocator keeps. As in `fs-messenger-mail`.
* **Text so far lives in memory** until the turn ends. The last 32 ended turns stay readable as `LiveTurn`, and a program restart forgets them, and the history.
* **A listener more than 1024 events behind** misses pieces and reads `LiveTurn`, which is what the contract asks for an offset above what it holds.
* **`Conversation.message` does not return the text so far,** as the contract asks, because the message is not stored yet.
* **Live `created` has `sender_kind: unknown`:** a provider cannot ask the lookup, and it must not claim. The stored message has the guard's word.
* **`Account.encryption_ready` reads true,** because the provider can plan, though each plan is refused. A test pins it.
* **Idle 120 seconds is a guess:** a model that takes minutes over its prompt on a slow machine fails unless a program raises it. Nothing was measured.
* **`max_tokens` 16 000 is a guess too,** and the same for all models. Some may refuse it.
* **A hostile server can make a turn cost** up to 16 MiB, 30 minutes, and the tokens it claims in `usage`, which go into the total as reported.
* **A model that repeats itself endlessly is not detected as such;** the same delta twice is legal. Size and turn time end it.
* **HTTP/1.1 only.** Not checked whether a real server wants more.

## Wanted

From `fs-messenger-core`; none of this was changed here.

1. **A way past the gate, or not:** the owner's decision above. Without it nothing here runs.
2. **A turn's way into the store and the feed:** create an answer as `sending`, add text, end it as `sent` or `failed`, with `Event::Text` on `Messenger::subscribe` and `text.stream` supported. Today `Event::Text` has no emitter a provider can reach, `announcements` is private and `passes` drops everything but tool calls, and `Conversation.message` cannot return the text so far. This crate's live side would then be one implementation behind that, with the stored id from the start.
3. **A stored `failed` for an answer.** `RawMessage` has no state; an answer that broke off reads `delivered`, and only its name says otherwise. `tests/hostile_stream.rs` pins this.
4. **Which conversation.** `Outgoing` names no conversation id, and `Ingested` neither. `host.rs` finds it by walking the conversation list for its newest message, 8 pages at most; if another message overtakes in that moment, the send fails, which at least fails safe.
5. **Usage that adds.** `record_usage` replaces, so a total is read and written back, unlocked. `context_size` stays 0: no way to learn it was used.
6. **`encryptionReady` that means something for a provider whose plans are refused.**
7. **`SenderLookup` for a provider, read only,** if the live `created` is to carry `ai`; not needed once 2 exists.

From `fs-messenger-guard`: nothing. `Gate::admit_message` was enough.

## Open points

1. ADR 0030 and the wire bytes, under "Not built".
2. Whether to ask for `fallbacks` on a refusal, which needs a beta header and is model-specific.
3. A limit across conversations for a server with few slots.
4. The live feed uses a `broadcast::Receiver`, not the contract's `Listener`; that goes away with "Wanted" 2.

## Crates

Run-time, new to this repo: `reqwest` 0.13 (`rustls-no-provider`, no default features), `rustls` 0.23 with ring, `rustls-native-certs`, `zeroize`, as in `fs-messenger-mail`. The rest comes through them. Licences of everything new in `Cargo.lock` compared with `origin/guard`, from `cargo metadata`:

| Licence | Crates |
|---|---|
| MIT OR Apache-2.0, and `MIT/Apache-2.0` | `reqwest`, `url`, `idna`, `idna_adapter`, `percent-encoding`, `form_urlencoded`, `httparse`, `ipnet`, `tokio-rustls`, `rustls-pki-types`, `rustls-platform-verifier`, `rustls-platform-verifier-android`, `openssl-probe`, `zeroize`, `atomic-waker`, `futures-channel`, `simd_cesu8`, `web-sys`, `wasm-bindgen-futures`, `core-foundation`, `security-framework`, `security-framework-sys`, `jni`, `jni-macros`, `jni-sys`, `jni-sys-macros`. Dev only: `rcgen`, `asn1-rs`, `asn1-rs-derive`, `asn1-rs-impl`, `der-parser`, `oid-registry`, `rusticata-macros`, `x509-parser`, `yasna`, `bit-vec` |
| MIT | `hyper`, `hyper-util`, `http-body`, `http-body-util`, `tower`, `tower-http`, `tower-layer`, `tower-service`, `try-lock`, `want`, `combine`, `schannel`. Dev only: `pem`, `data-encoding` |
| Apache-2.0 | `sync_wrapper` |
| **Apache-2.0 OR ISC OR MIT** | `rustls`, `hyper-rustls`, `rustls-native-certs` |
| **ISC** | `rustls-webpki`, `untrusted` |
| **Apache-2.0 AND ISC** | `ring` |
| **Unlicense OR MIT** | `same-file`, `walkdir`, `winapi-util` |
| **CDLA-Permissive-2.0** | `webpki-root-certs`: in the lock through `rustls-platform-verifier`, on other platforms. `cargo tree -i` finds it in no normal dependency of this crate on Linux |

The ones in bold go beyond "MIT or Apache-2.0", which the guard's `DESIGN.md` names as the rule: for the owner under ADR 0015. `rustls`, `ring` and `rustls-webpki` are already what `fs-messenger-mail` uses. Dev also: `tokio-rustls`, `tempfile`, `ed25519-compact`.
