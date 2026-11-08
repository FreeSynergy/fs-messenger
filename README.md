# FreeSynergy Messenger — say something to someone

**One place for mail, chat and agents, and a message that is only sent if it can be sent encrypted.**

FreeSynergy Messenger treats a message as a message, whichever network brought it. Mail, chat and AI agents are different connectors under one program. You read, answer, search and file in one place, and the connectors do the talking to the outside world.

What is different here: it is encrypted only. If no way to encrypt a message to all its recipients exists, it is not sent, and there is no override. Incoming messages are marked with what the connector really knows, and anything not proven end-to-end is never shown as secure. The original of every mail is kept byte for byte. AIs are participants, not hidden listeners. Each one is marked, none acts without a release from you, and there is an emergency stop that wins over everything.

## What it can do today

Latest tag: `v0.4.0`.

- A message model for mail, chat and agents; conversations, paging that does not jump when new mail arrives, unread marks, drafts, search, pin, move, flag, forward.
- Encrypted-only sending, with a clear refusal naming the recipients that cannot be reached.
- Messages stored with their original; very large mails leave a visible stub, not a hole.
- A mail connector (`fs-messenger-mail`): fetch over IMAP or JMAP, send over SMTP or JMAP, OpenPGP with Autocrypt. Tested against a local Stalwart test server only.
- An AI guard (`fs-messenger-guard`): an invitation for each AI, a release per AI and per kind of action, questions you answer for risky steps, secrets never given to an AI, and an emergency stop.
- Try the core: `cargo run -p fs-messenger-core --example demo`.

## What is not there yet

- No window, no server, no chat connectors (Matrix and others), and no Telegram.
- Messages stay on the device they arrived on; "on all devices" is not met, on purpose, until the key vault exists.
- Editing and reacting to a message, scheduled sending, mail rules, HTML bodies, key discovery, STARTTLS and S/MIME answer "not possible".
- Account passwords cannot be stored yet because there is no key vault.
- The rule that all members must agree before an AI joins a conversation is not built. The guard's rules are provisional.
- The interface is a draft with no consumer yet.

## Part of FreeSynergy

Messenger is where you talk, and later where you talk to an AI that works with the other programs. FreeSynergy is a family of programs that are free for everyone, forever, and that keep your data yours. The documentation, the manifesto and the overall plan live in [fs-documentation](https://github.com/FreeSynergy/fs-documentation); the reading version is at <https://freesynergy.github.io/fs-documentation/>. Start with the [manifesto](https://freesynergy.github.io/fs-documentation/manifesto/) if you want to know why we do this.


## For developers

- Workspace crates: `fs-messenger-core` (core, no GUI, no network), `fs-messenger-guard` (the AI guard), `fs-messenger-mail` (the mail connector).
- The contract is `messenger.capnp` in [fs-interfaces](https://github.com/FreeSynergy/fs-interfaces) (draft). Messenger owns it.
- Build and test: `cargo build --workspace` and `cargo test --workspace`. The mail tests need the test servers in `scripts/test-servers`.
- Design: [DESIGN.md](DESIGN.md); each of the other two crates has its own `DESIGN.md`.
- Licence: MIT OR Apache-2.0.
