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
| Incoming: decrypt, verify (combined, or `multipart/signed` at the root or just inside), `openpgp` only if really decrypted; `raw.undecrypted`, `raw.signature` | done |
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
| `provider/read.rs` | Template method | classify, open, verify; writes nothing; all of it on one blocking thread |
| `provider/ids.rs` | Adapter | reversible encoding, because the core refuses `\|` and a mail may legally hold it |
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
2. **Same key: it only refreshes.** The offered form is taken if it carries more revocations (of the key or of a subkey), or if it is newer, counting subkey bindings, so a new subkey is learned. A forger can only replay or strip, since he cannot add self-signatures: so a form with fewer revocations is never taken.
3. **Another key: replaced only with proof** (`AutocryptRotated`). The proof is that the new key was offered in an `Autocrypt` header **inside** an encrypted payload, whose signature **covers that header** and verifies with the **currently known** key, and that key is neither revoked nor expired. An outer header proves nothing, even on a mail with a good signature, and neither do the headers around a `multipart/signed`: the signature covers the signed part only. Both attacks are tests, the second on the real server.
4. **Without proof: the known key stays**, the offer is remembered as `candidate`, never encrypted to, readable through `MailProvider::peer`. Only a person replaces it: `accept_candidate` with the fingerprint she compared, refused if the candidate has changed since, or `import_peer_key`.

Consequences to decide on: a peer who lost his key cannot prove anything, so he stays in conflict until a person acts. A known key that expired is refused in the plan, and its successor is only a candidate unless the old one signed it first. A rebuilt device replays old mail and reaches the same state only if the mail arrives in order.

The first contact: nothing leaves unencrypted, so asking for a key by mail is not possible. A key comes from a mail someone else wrote, or `import_peer_key`.

## Decisions

* **Provisional: what dedupes.** The core takes only provider-issued aliases (`mail-local`: account, mailbox, UIDVALIDITY and UID, or JMAP email id). `Message-ID` only links. A UIDVALIDITY change therefore makes mail new again. **The same mail over IMAP and over JMAP has two different aliases**, so changing the strategy of an account stores it twice.
* **Withholding the link.** The core matches a link against our own receipts if the sender is the account's address, and a `From` proves nothing. So a mail in the inbox claiming to be from us only links if our own key signed it; otherwise a stranger could hide his mail, or claim one of ours, with a known `Message-ID`. Tested on the server. The core's rule alone does not close this.
* **The sent copy.** SMTP keeps none, so the provider stores it through the fetch strategy (`APPEND`, `Email/import`). JMAP imports it in the same request and destroys it if the submission is refused. If the copy fails after the mail left, the send still counts and a warning is logged: failing would make a retry send twice.
* **Only the root decides.** `multipart/encrypted` inside something else is plain. Otherwise a sender could put readable text around a real ciphertext and have it marked `openpgp`. A literal packet in an encrypted envelope is not decrypted either.
* **What a signature vouches for (PROVISIONAL).** Only what it covers. `SignatureCheck::Valid` needs three things: the signature verifies, with a key known **before** this mail, over a part that names the sender in its own `From`. So a key offered in the same mail reads `NotVerified` on first contact, and **a signature over a bare body, which is what plain gpg writes without protected headers, reads `NotVerified`**: anyone can put a bare signed body under any `From` (surreptitious forwarding). Headers next to `Valid` are the covered ones. Without `Valid` or a declared `hp`, inner headers are not believed. `hp` counts only as `cipher` or `clear` (RFC 9788 section 4.1). A revoked or broken key (DSA, Elgamal, RSA under 2048) verifies nothing; an expired one still does, for what it signed while it lived, and rPGP does not say when that was.
* **Undecryptable:** `none`, `raw.undecrypted`, empty body, outer subject, original kept. That includes a plaintext over the limit: compression can expand 1.4 MB to 1 GiB, so the reader stops at the core's limit (`with_max_message_bytes`). A key store failure fails the fetch instead, so the cursor stays: otherwise a passing failure would mark readable mail as unreadable for good.
* **`transport-only` is never claimed.** We measure TLS on our own leg to the server, not the way the mail travelled. Plain mail is `none`.
* **Header Protection, baseline.** The outer `Subject` is `[...]`; `From`, `To`, `Cc`, `Date`, `Message-ID`, `In-Reply-To`, `References` stay outside. **So who writes to whom, and which mail answers which, is visible to servers.** The attachment names and the subject are not. Each sender's own `Autocrypt` is written on both levels: the inner one is the proof, the outer one reaches a reader who cannot open the mail.
* **Hidden recipients** get an anonymous session key packet, so that the others cannot read their key id; they are in no header, inner or outer. **The packets can be counted, so every reader sees THAT someone is hidden, and how many.** One ciphertext per hidden recipient would close that; not built. Every reader's client tries its key on such a packet: a small cost.
* **Provisional: v6 certificates in Autocrypt v1 headers.** The v2 draft is not final. What other OpenPGP programs do with them: "Interoperability" below.
* **No subject is no subject.** `Outgoing::subject()` empty means no `Subject` inside; outside it stays `[...]`.
* **Ids with `|` or control characters** are `%`-encoded on the way to the core and decoded on the way out, because the core refuses them in alias parts and a legal mail must not be lost (ADR 0030). The ids the core shows are the encoded ones. An id with a control character never goes back into a header: that would be header injection. An id over the core's length limit is dropped as a link; as a thread root it falls back to a thread of the mail's own.
* **Which `Message-ID` links:** the inner one if inside is believed. The outer one is covered by nothing, so a replay of a signed ciphertext can carry any.
* **JMAP paging by `anchor`.** A position skips a mail when an earlier one is deleted between pages. If the anchor itself is deleted, the walk starts over from 0, which re-downloads and dedupes.
* **A failure in the sent mailbox** does not cost what the inbox delivered: a warning, and the sent cursor stays. Shared by both strategies; tested over IMAP only, because no real server fails on request and there is no fake JMAP server here.
* **Limits at the strategies.** Of a mail over the limit only the start is read, one byte past it (`BODY.PEEK[]<0.n>`, a JMAP download stopped early), and `FetchedMail::truncated` says so. It is not opened and offers no key. The core sees a mail over its limit and keeps a visible stub with `too_large`, without the bytes; **the mail itself stays only on the server.** The size in the stub is therefore about the limit, not the real size. `with_max_message_bytes` must match the core's limit: a lower one here would store the cut-off start as an original. JSON from a JMAP server is cut at 32 MiB. IMAP calls run under `ImapConfig::timeout`; lettre and reqwest have their own.
* **`jmap-client` is not used.** It builds its HTTP client inside and has only a general "accept invalid certificates" switch for a self-signed server, and it follows the session document's URLs. Ours refuses a session that advertises anything but `https`, or another origin than the configured one: the document decides where the password goes next. `rebase_on_session_origin` moves the URLs onto the configured origin; the test server needs this, it advertises `http://` even when asked over HTTPS. `other_credential_origins` names further origins for one account, empty by default.
* **Keys in a store of their own.** `FsKeyStore` works on any `Store`, but a store that synchronises to a hub would carry the secret there, and `fs-store`'s log keeps old versions until compaction. A program should open one encrypted store for keys. Behind `dyn Store`, a store opted out of the sensitive-data rule cannot be told apart; `open_turso` refuses it. The secret has no passphrase: it relies on the encrypted store.
* **What is wiped, exactly.** Wiped on drop: `Password` (`Zeroizing<String>`), the secret packets inside `SecretCert` (rPGP's `ZeroizeOnDrop`), and the binary buffers `FsKeyStore` decodes into or encodes from. **Not wiped:** the base64 `String` and the `serde_json::Value` the key passes through in `FsKeyStore`; the copy `lettre`'s `Credentials` takes and the header `reqwest` builds, for every login; whatever the allocator kept from moved buffers; everything in fs-store. **The secret key sits in plain JSON in the activity log of its store** (encrypted at rest with the store), because fs-store logs before and after images of every `put` and offers no way around it. `Debug` of an `Activity` prints it, and `Store::subscribe` hands it to every listener. `known_limit_the_own_key_is_in_the_activity_log_of_its_store` pins this. `Debug` of `Password`, `SecretCert` and `PublicCert` shows no secret.
* **The certificate of a test server** can only be trusted under the `test-trust` feature, which only the dev-dependency turns on. Exactly that certificate, host name still checked.

## Interoperability (measured 2026-11-06)

Facts only. "Mail" is what `Draft.send` handed the strategy, or a hand-written RFC 3156 wrapper read through `fetch`; the transport was replaced, no mail travelled over a network.

**Changed since this was measured.** The rule of "What a signature vouches for" came after 9179bea, and `inspect` is gone: the state is now `raw.signature`. The GnuPG cases in `tests/interop_gnupg.rs` were adapted and run again (see below). `Valid` in the tables below means the old rule, where a bare body counted. **Under the current rule a signature from gpg over a bare body reads `NotVerified`, and only a body naming its `From` reads `Valid`; a first mail offering its own key reads `NotVerified`.** `UNDECRYPTED_MARK` no longer exists: the body is empty and `undecrypted` is set. The RNP cases were a one-off and were **not** run again.

**GnuPG cases:** `tests/interop_gnupg.rs`, `#[ignore]`, run on 2026-11-06 at commit 9179bea plus that file, 3 of 3 passed. **RNP cases:** a one-off test with the same helpers, not in the repository (the file limit of 300 lines); it ran against the extracted packages named below, from an empty directory.

### Tool versions

| Tool | Version, source |
|---|---|
| GnuPG | `gpg (GnuPG) 2.4.9`, `libgcrypt 1.12.2-unknown`, package `gnupg2-2.4.9-18.fc45.x86_64`; throwaway `GNUPGHOME` for every run, `--no-auto-key-locate` |
| rPGP | `pgp` 0.21.0 |
| RNP (Fedora) | `rnp 0.18.1`, `Backend version: 3.11.1`; `rnp-0.18.1-5.fc45`, `librnp-0.18.1-5.fc45`, `botan3-3.11.1-5.fc45`, `libsexpp-0.9.2-4.fc45`, `jitterentropy-3.7.0-5.fc45`; RPM signatures checked with `rpm -K`, unpacked with `rpm2cpio`, not installed |
| RNP (of Thunderbird) | `/usr/lib64/thunderbird/librnp.so` of `thunderbird-librnp-rnp-158.0-2.fc45`, version string `0.18.1.MZLA.158.0.botan.3.11.1`, under the `rnp` programs of the Fedora package; its log paths read `thunderbird-158.0/comm/third_party/rnp/...` |
| Thunderbird | not run; `thunderbird-158.0-2.fc45` requires `(thunderbird-librnp(x86-64) or librnp(x86-64))` |

### Matrix: GnuPG 2.4.9

| Case | Result | What it said |
|---|---|---|
| A. Our v6 certificate, `--import` | refused, exit 2, no key held | `gpg: packet(6) with unknown version 6` / `gpg: read_block: read error: Invalid packet` / `gpg: import from '[stdin]' failed: Invalid keyring` / `gpg: Total number processed: 0` |
| A. `--encrypt --recipient` it afterwards | refused | `gpg: us@example.org: skipped: No public key` / `gpg: [stdin]: encryption failed: No public key` |
| B. gpg's v4 key (ed25519/cv25519) in our rules | `PublicCert::from_binary` and `import_peer_key` accept it; the plan refuses it | `usable_encryption_key`: `NoModernEncryption`, "it does not support SEIPDv2". Plan: `EncryptionPlan { encryption: None, missing_keys: ["peer@example.org"] }`. Send: `EncryptionRefused { encryption: None, missing_keys: ["peer@example.org"] }`, nothing reached the strategy. The key says `hashed subpkt 30 len 1 (features: 07)`, `hashed subpkt 34 len 1 (pref-aead-algos: 2)` |
| C1. Our mail to a v6 peer | gpg holds no key (A); `--decrypt` exit 2 | `gpg: packet(1) with unknown version 6` |
| C2. Our mail to a v4 key by rPGP with Ed25519Legacy/Curve25519Legacy, advertising SEIPDv2; its secret imported into gpg (`secret key imported`) | the plan says `Openpgp`, the mail is sent; `--decrypt` exit 2. The same with a hidden recipient | `gpg: packet(1) with unknown version 6` |
| C3. The same with v4 Ed25519/X25519 (algorithms 27 and 25) | the plan says `Openpgp`; gpg does not take the secret; `--decrypt` exit 2 | `gpg: key ...: secret key info missing`; `gpg: packet(1) with unknown version 6` |
| C4. gpg's own v4 key, past the plan (`sign_and_encrypt` called directly; **no path in the product does this**) | `--decrypt` exit 2 | `gpg: packet(1) with unknown version 6` |
| D. gpg signs and encrypts to `US`, whose key in our store is gpg's v4 key or a v4 key by rPGP (`put_own_key` called by the test; **`ensure_own_key` only ever generates v6**), default options or `--rfc4880` | decrypted, `Openpgp`, body correct. Signature: known key `Valid`; unknown key `Unverified`; key offered in the mail's `Autocrypt` header `Valid`, and `peer` stays `None` afterwards (the rule ignores a key it cannot encrypt to) | |
| D. The same, `--force-ocb` | not decrypted: `None`, `NotSigned`, body is `UNDECRYPTED_MARK`, original kept | `decrypt`: `openpgp: GnuPG's AEAD packet (type 20) is non-standard, 'DecryptionOptions' allows opting into it` |
| D. The same, `--digest-algo SHA1` | decrypted, `Openpgp`; signature `Unverified` even with the known key | |
| D. Signature inside the encryption (RFC 3156 section 6.1) | `Openpgp`, `Valid` | |
| D. `multipart/signed` at the root (RFC 3156 section 5), no encryption | `None`, `Valid` | |
| D. gpg to our product key (v6) | not possible: gpg cannot import it (A) | |
| E. v5 key by gpg (`Key-Version: 5`, `Subkey-Version: 5`) | gpg lists `version 5`; `PublicCert::from_binary` and `SecretCert::from_binary` refuse it | `openpgp: no matching packet found` |
| F. Our `Autocrypt` header: value of 816 octets, `keydata` byte-identical to A | refused like A | the same four lines as A |

**Packets seen, `gpg --list-packets` on real output** (`tag` as gpg prints it; versions gpg does not print were read from the first body octet of each packet, by a small parser in an earlier draft of the test):

| Written by | Tag and version | gpg's listing |
|---|---|---|
| our builder, every case | tag 1 version 6 (once per recipient), tag 18 version 2 | `:pubkey enc packet: [unknown version]`, `:encrypted data packet: [unknown version]`, `gpg: encrypted_mdc packet with unknown version 2`, followed by packets misread from ciphertext |
| our v6 certificate, also in the header | tags 6, 2, 13, 2, 14, 2; 6, 14 and 2 version 6 | `:key packet: [unknown version]`, `:signature packet: [unknown version]`. Six packets, the direct-key signature being the extra one |
| gpg, default, `--rfc4880`; also default to a v5 key (in an earlier draft of the test, not in the committed one) | tag 1 version 3, tag 18 version 1 | `:pubkey enc packet: version 3`, `:encrypted data packet:` with `mdc_method: 2` |
| gpg, `--force-ocb` | tag 1 version 3, tag 20 version 1 | `:aead encrypted packet: cipher=9 aead=2 cb=16` |

**GnuPG's AEAD:** this build read tag 18 version 2 as unknown, and wrote tag 20 only when forced. By hand in a throwaway home, on a key of its own (`features: 07`): no option and `--rfc4880bis` wrote tag 18 version 1 (`mdc_method: 2`), `--force-aead` and `--rfc4880bis --force-ocb` wrote `:aead encrypted packet:`. `gpg --dump-options` names `--force-ocb`, `--force-aead`, `--aead-algo`, `--personal-aead-preferences`, `--rfc4880bis`, nothing with 9580. Tag 20 is not in RFC 9580, whose flags 0x02 and 0x04 are `Reserved`; `features: 07` sets them, 0x08 is not set.

**Why the default here is tag 18:** Fedora's `gnupg2.spec` (rawhide, read 2026-11-06, not the fc45 source package) carries `0006-Do-not-use-OCB-mode-even-if-AEAD-OCB-key-preference-.patch`, whose text is "Ignore AEAD: OCB key preference unless --force-ocb is set", and `0007-Revert-the-introduction-of-the-RFC4880bis-draft-into.patch`. Upstream `g10/encrypt.c`, `use_aead`, tag `gnupg-2.4.9` (commit 21c7d29d, 2025-12-30): `/* If all keys support AEAD we can use it.  */ return select_aead_from_pklist (pk_list);`. An unpatched build was not run.

### Matrix: RNP 0.18.1

The same results with both libraries named above: the labelled result lines of the two runs are identical once fingerprints are masked, and the log messages occur the same number of times (their paths and line numbers differ). `RNP_LOG_CONSOLE=1` for the reasons.

| Case | Command | Result | What it said |
|---|---|---|---|
| Our v6 certificate | `rnpkeys --import` | refused | `wrong key packet version` / `failed to parse key pkt at 0` / `Import finished: 0 keys processed, ...` |
| Our mail to a v6 peer | `rnp --decrypt` | fails | `wrong packet version` / `Failed to parse PKESK, skipping.` / `unknown SEIPD version: 2` |
| Our mail to a v4 key by rPGP advertising SEIPDv2, secret imported (`2 new secret keys`) | `rnp --decrypt` | fails | the same three lines |
| A key by `rnpkeys --generate-key --expert`, choice 22 (EDDSA + ECDH) | our rules | v4; hashed subpackets 33, 2, 9, 27, 11, 21, 22, no 30 (Features); `NoModernEncryption` | |
| `rnp --encrypt --sign` to `US`, whose own key in our store is an RNP key (by test), sender's RNP key known | our reading | tag 1 version 3, tag 18; `Openpgp`, `Valid`, body correct | |
| The same, `--aead=OCB` | our reading | tag 1 version 3, tag 20 version 1, AES-256, OCB; not decrypted, `NotSigned`, mark | the same `decrypt` error as with gpg |

### Thunderbird and RNP, from source

No statement below ran in Thunderbird.

* comm-central at 10b5e74aeb79 (2026-11-06, `mail/config/version.txt` 162.0a1), `third_party/README.rnp`: "[tag v0.18.1]", "This is the copy that Thunderbird uses by default and ships in releases."
* The same revision, `third_party/rnp/moz.build`: sets `ENABLE_AEAD`, and has no `ENABLE_CRYPTO_REFRESH`; `third_party/rnp/src/lib/config.h.in` has `#cmakedefine ENABLE_CRYPTO_REFRESH`.
* The same revision, `mail/extensions/openpgp/content/modules/RNP.sys.mjs`, line 4304: `// Don't use AEAD as long as RNP uses v5 packets which aren't` / `// widely compatible with other clients.`, then `rnp_op_encrypt_set_aead(op, "NONE")`.
* RNP tag v0.18.1 (2025-11-21), `CMakeLists.txt` line 82: `option(ENABLE_CRYPTO_REFRESH "Enable crypto-refresh support (v6)")`. Fedora's `rnp.spec` (rawhide) passes no such flag. Neither is a v6 build, as measured above.

### RFC 9580 (July 2024), <https://www.rfc-editor.org/rfc/rfc9580.txt>

* Section 5.5.2: "OpenPGP implementations SHOULD create keys with version 6 format. Version 4 keys are deprecated; an implementation SHOULD NOT generate a version 4 key but SHOULD accept it."
* Section 5.1.1: "A version 3 PKESK packet precedes a v1 SEIPD packet"; "A v3 PKESK packet MUST NOT precede a v2 SEIPD packet".
* Section 5.1.2: "A v6 PKESK packet precedes a v2 SEIPD packet"; "v6 PKESK packet MUST NOT precede a v1 SEIPD packet".
* Section 10.3.2.1: "implementations MUST NOT precede a v2 SEIPD payload with either v3 PKESK or v4 SKESK packets." Its Table 26 lists v1 SEIPD with v3 PKESK and v2 SEIPD with v6 PKESK, both "Yes" under "Generate?".
* Section 13.7: "If all recipient keys indicate support for a version 2 Symmetrically Encrypted and Integrity Protected Data packet in their Features signature subpacket (Section 5.2.3.32), if all recipient keys are version 6 keys without a Features signature subpacket, or the implementation can otherwise infer that all recipients support v2 SEIPD packets, the implementation SHOULD encrypt using a v2 SEIPD packet." "If one of the recipients does not support v2 SEIPD packets, then the message generator MAY use a v1 SEIPD packet instead." "Implementers should implement AEAD (v2 SEIPD and S2K usage octet 253) promptly and encourage its spread."
* Section 5.2.3.32, Table 11: 0x01 v1 SEIPD, 0x02 and 0x04 "Reserved", 0x08 v2 SEIPD.
* Section 9.1: "Implementations MUST implement Ed25519 (27) for signatures and X25519 (25) for encryption."
* Section 9.2: "The deprecated OIDs for Ed25519Legacy and Curve25519Legacy are used only in version 4 keys and signatures. Implementations MAY implement these variants for compatibility with existing version 4 key material and signatures."
* Section 9.6: "Implementations MUST implement OCB."

### Autocrypt level 1 (1.1.0), <https://autocrypt.org/level1.html>

No key version is named. Section "OpenPGP Based key data": "The keydata sent by an Autocrypt-enabled Level 1 MUA MUST consist of an OpenPGP “Transferable Public Key” containing exactly these five OpenPGP packets"; "A Level 1 MUA MUST be capable of processing and handling Ed25519 public keys for signatures, as well as Cv25519 for encryption. It MAY support other OpenPGP key formats found in an Autocrypt header". Section "Secret key generation and storage": "The MUA SHOULD generate and store a Ed25519 plus Cv25519 secret key", "(RFC 4880 “Transferable Public Key”)". Ours carries six packets (table above).

### `cfg_block` 0.1.1 (through `turso_core`)

No `license` field: `Cargo.toml.orig` in the source tarball (`~/.cargo/registry`) has `license-file = "LICENSE"`. That `LICENSE`, 553 bytes, is the licence notice, not the full text: "Copyright 2022 Trevor Gross", "Licensed under the Apache License, Version 2.0". The tarball's sha256 `18758054972164c3264f7c8386f5fc6da6114cb46b619fd365d4e3b2dc3ae487` equals the checksum in the crates.io API and in `Cargo.lock`. crates.io API: `license: "non-standard"` for 0.1.1, repository `pluots/cfg_block`, whose `LICENSE` on `main` has the same text; GitHub reports `NOASSERTION`. The dependency is unchanged.

### Not measured

Thunderbird itself; Mozilla's own builds; RNP built with `ENABLE_CRYPTO_REFRESH`; `sequoia-octopus-librnp`, which Fedora offers as a `librnp` provider; an unpatched GnuPG, or any other version; any other OpenPGP program; reading by a real mail client's MIME code (the armored part was handed to the tools). Whether gpg reads a v1 SEIPD message from rPGP: our code cannot write one and none was built. Why rPGP reads gpg's SHA-1 signature as `Unverified` was not traced in its source.

## Open points

Wanted from `fs-messenger-core`, worked around here:

1. No field or event for a key conflict: `MailProvider::peer(address).candidate`.
2. `RawMessage` has no way to say "over the limit, this size": the strategies read limit + 1 bytes so that the core's own size check fires. A `too_large: Option<u64>` would give the real size without the download.

Wanted from `fs-store`:

1. A way to store an object without its images in the activity log, or a log that keeps sensitive payloads out, for secret keys. See "What is wiped".

Code:

1. A mail under the limit is held in memory whole, up to 50 per mailbox per fetch, and a provider fetch can so hold 100 times the limit. `async-imap` buffers a literal before we can stop it, so a server that lies about `RFC822.SIZE` is not bounded; JMAP is, by the early stop.
2. A signature does not expire the trust in it: an expired signer's old signatures read `Valid`.
3. A flag change on an old mail makes JMAP download it again; nothing is stored twice.
4. Stalwart drops a second delivery with the same `Message-ID` and the same subject (observed on 1.0.3, not read in its source): the mail never reaches us, on any fetch.
5. Weak keys: DSA, Elgamal and RSA under 2048 are refused. Other curves and algorithms rPGP parses pass. The RSA path has no test: rPGP will not generate such a key.
6. `ImapConfig::timeout` covers one whole call: a large mailbox on a slow line can run into it, and the next call starts from the same cursor.

## Crates

`mail-parser` and `mail-builder` pinned with `=` (ADR 0018). `pgp`, `lettre`, `async-imap`, `reqwest`, `rustls` with ring. TLS is rustls only.
