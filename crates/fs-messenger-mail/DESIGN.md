# fs-messenger-mail – Design

The mail provider of FreeSynergy.Messenger: one account's mail behind the core's `Provider` trait. No GUI, no RPC, no server.

## State

Version 0.1.0, not tagged. Tested against Stalwart 1.0.3 from `scripts/test-servers` only.

| Area | State |
|---|---|
| Fetch: `MailFetch`, IMAP (`async-imap`) and JMAP (own small client), cursor per mailbox | done, against the server |
| Submit: `MailSubmit`, SMTP (`lettre`) and JMAP `EmailSubmission` | done, against the server |
| Parse and build: `mail-parser`, `mail-builder`; original byte for byte; JMAP-style fields; threading; attachments; `bodyHtml` empty | done |
| OpenPGP (rPGP): RFC 3156 section 6.2 combined form, v6 keys, SEIPDv2 AES-256 OCB, RFC 9788 Header Protection, to all recipients and the sender | done, between our own two ends only |
| Autocrypt, own: send the header, learn from incoming, the rule below | done, **provisional** |
| Incoming: decrypt, verify (combined, or `multipart/signed` at the root or just inside), `openpgp` only if really decrypted | done |
| Keys: `KeyStore`, `FsKeyStore` over `fs-store` as sensitive objects | done |

### `Unsupported`, or not built

| What | How it shows |
|---|---|
| Push (IMAP IDLE, JMAP push) | `message.push` reported unsupported; call `fetch` |
| Key discovery (WKD, key servers), Autocrypt gossip | `key.discovery` reported unsupported; keys come from Autocrypt or `import_peer_key` |
| STARTTLS | `transport.starttls` reported unsupported: implicit TLS only, the test server has no such port |
| S/MIME | `encryption.smime` reported unsupported |
| Outgoing SEIPDv1, so a v4 key that does not advertise SEIPDv2 | refused in the plan, `NoModernEncryption` |
| Autocrypt setup message, several own keys, key rotation of one's own key as a flow | not built; an own key that is replaced takes the reading of older mail with it |
| Legacy display part of RFC 9788, in or out | not built; incoming, such a part shows as text |
| Inline PGP | stays plain text, `none` |
| DKIM, SPF, DMARC | not checked; a `From` is taken as written, which is why it proves nothing here |
| Reading the JMAP `threadId` | not used: threads come from headers, so that IMAP and JMAP agree |

## Pattern map (ADR 0043)

| Module | Pattern | Why |
|---|---|---|
| `provider/facade.rs` | Facade + Strategy | the core sees `Provider`; behind it two small strategies and keys |
| `provider/send.rs` | Template method | check again, build, encrypt, submit, keep the copy |
| `provider/read.rs` | Template method | classify, open, verify; writes nothing, so `inspect` can use it too |
| `provider/receive.rs` | Adapter | mail to `RawMessage`; alias and link decided here |
| `fetch/*`, `jmap/fetch.rs` | Strategy, value object for the cursor | bytes only, so that both behave the same |
| `submit/*`, `jmap/submit.rs` | Strategy | finished bytes only: a strategy never sees a plaintext |
| `jmap/client.rs`, `jmap/session.rs` | Facade, Specification | what a server's session document may make us do |
| `mime/classify.rs`, `autocrypt/rule.rs`, `openpgp/usable.rs` | Specification | security rules as pure functions |
| `mime/build.rs` | Builder | the two forms of a mail |
| `keys/*` | Repository + Adapter | the provider does not know `fs-store` |
| `secret.rs`, `openpgp/cert.rs` | Newtype | a secret cannot be printed |
| `tls.rs` | Strategy | whose certificates count; there is no "accept invalid" |

## The Autocrypt rule (PROVISIONAL)

Who may change which key we encrypt to. Stated here because Autocrypt 1.1 itself says "the newest header wins", and that lets anyone who can send a mail replace a key.

A header counts only if its `addr` is the sender, it is the only one for that sender, the certificate parses, its self-signatures verify, and it is usable now.

1. **Nobody known: first seen wins** (trust on first use, `AutocryptFirstSeen`). Nobody checked it. **This is a real limit:** a forger who writes first is believed, and the real key then shows up as the conflict. DKIM is not checked, and it would only narrow this.
2. **Same key: it only refreshes.** The offered form is taken if it is newer, or if it adds a revocation; a forger can only replay older forms, since he cannot add self-signatures. A revocation is never given up.
3. **Another key: replaced only with proof** (`AutocryptRotated`). The proof is that the new key was offered in an `Autocrypt` header **inside** an encrypted payload, whose signature verifies with the **currently known** key. An outer header proves nothing, even on a mail with a good signature, because the signature does not cover it; the server test takes a real signed mail and puts a forged outer header on it.
4. **Without proof: the known key stays**, the offer is remembered as `candidate`, never encrypted to, readable through `MailProvider::peer`. Only a person replaces it: `accept_candidate`, or `import_peer_key`.

Consequences to decide on: a peer who lost his key cannot prove anything, so he stays in conflict until a person acts. A known key that expired is refused in the plan, and its successor is only a candidate unless the old one signed it first. A rebuilt device replays old mail and reaches the same state only if the mail arrives in order.

The first contact: nothing leaves unencrypted, so asking for a key by mail is not possible. A key comes from a mail someone else wrote, or `import_peer_key`.

## Decisions

* **Provisional: what dedupes.** The core takes only provider-issued aliases (`mail-local`: account, mailbox, UIDVALIDITY and UID, or JMAP email id). `Message-ID` only links. A UIDVALIDITY change therefore makes mail new again. **The same mail over IMAP and over JMAP has two different aliases**, so changing the strategy of an account stores it twice.
* **Withholding the link.** The core matches a link against our own receipts if the sender is the account's address, and a `From` proves nothing. So a mail in the inbox claiming to be from us only links if our own key signed it; otherwise a stranger could hide his mail, or claim one of ours, with a known `Message-ID`. Tested on the server. The core's rule alone does not close this.
* **The sent copy.** SMTP keeps none, so the provider stores it through the fetch strategy (`APPEND`, `Email/import`). JMAP imports it in the same request and destroys it if the submission is refused. If the copy fails after the mail left, the send still counts and a warning is logged: failing would make a retry send twice.
* **Only the root decides.** `multipart/encrypted` inside something else is plain. Otherwise a sender could put readable text around a real ciphertext and have it marked `openpgp`. A literal packet in an encrypted envelope is not decrypted either.
* **Undecryptable:** `none`, body = `UNDECRYPTED_MARK`, outer subject, original kept. A key store failure fails the fetch instead, so the cursor stays: otherwise a passing failure would mark readable mail as unreadable for good.
* **`transport-only` is never claimed.** We measure TLS on our own leg to the server, not the way the mail travelled. Plain mail is `none`.
* **Header Protection, baseline.** The outer `Subject` is `[...]`; `From`, `To`, `Cc`, `Date`, `Message-ID`, `In-Reply-To`, `References` stay outside. **So who writes to whom, and which mail answers which, is visible to servers.** The attachment names and the subject are not. Each sender's own `Autocrypt` is written on both levels: the inner one is the proof, the outer one reaches a reader who cannot open the mail.
* **Hidden recipients** get an anonymous session key packet, so that the others cannot read their key id; they are in no header, inner or outer. Every reader's client tries its key on such a packet: a small cost.
* **Provisional: v6 certificates in Autocrypt v1 headers.** The v2 draft is not final. Peers on GnuPG (no v6) cannot read what we send. **Not tested with any other OpenPGP program.**
* **Provisional: a subject from the first text line**, because `Outgoing` has none. It is encrypted like the rest.
* **`jmap-client` is not used.** It builds its HTTP client inside and has only a general "accept invalid certificates" switch for a self-signed server, and it follows the session document's URLs. Ours refuses a session that advertises anything but `https`; `rebase_on_session_origin` moves them onto the configured origin. The test server needs this: it advertises `http://` even when asked over HTTPS.
* **Keys in a store of their own.** `FsKeyStore` works on any `Store`, but a store that synchronises to a hub would carry the secret there, and `fs-store`'s log keeps old versions until compaction. A program should open one encrypted store for keys. Behind `dyn Store`, a store opted out of the sensitive-data rule cannot be told apart; `open_turso` refuses it. The secret has no passphrase: it relies on the encrypted store.
* **The certificate of a test server** can only be trusted under the `test-trust` feature, which only the dev-dependency turns on. Exactly that certificate, host name still checked.

## Open points

Wanted from `fs-messenger-core`, worked around here:

1. `RawMessage` has no field for the signature state or for "encrypted, not readable": `MailProvider::inspect(original)` derives both, with the keys held now, and the mark is body text.
2. No field or event for a key conflict: `MailProvider::peer(address).candidate`.
3. `Outgoing` has no subject: the first text line.
4. `Outgoing` gives the conversation and the parent, not the parent's `References`: we write `References: <root> <parent>`, which keeps the thread and loses the chain between.

Code:

1. A mail is held in memory whole, up to 50 per mailbox per fetch; a very large one is downloaded before the core refuses it (25 MiB). Such a refused mail is **not stored** and stays only on the server.
2. A signature does not expire the trust in it: an expired or revoked signer's old signatures read `Valid`.
3. A flag change on an old mail makes JMAP download it again; nothing is stored twice.
4. Stalwart drops a second delivery with the same `Message-ID` and the same subject (observed on 1.0.3, not read in its source): the mail never reaches us, on any fetch.

## Crates

`mail-parser` and `mail-builder` pinned with `=` (ADR 0018). `pgp`, `lettre`, `async-imap`, `reqwest`, `rustls` with ring. TLS is rustls only.
