# Messenger test servers

Disposable local servers, so that FreeSynergy's messenger connectors can be tested
against real software. Rootless podman, published on 127.0.0.1 only.

**All passwords and tokens here are throwaway test values.** They exist only in
these scripts and protect nothing but these containers. Never reuse them.

## Use

    bash up.sh       # start, first run also bootstraps Stalwart and creates the mail users
    bash check.sh    # end-to-end proofs, exits non-zero on failure
    bash down.sh     # remove our containers and volumes (fs-msgtest-*), all test data is lost

Requires podman, curl, jq, openssl and ss. `up.sh` and `down.sh` can be run repeatedly.

The Matrix users are registered by `check.sh`, not `up.sh`, so run `check.sh` once
before relying on them. A test may also register them itself with the token.

## How a test finds the servers

Source `env.sh`, or parse it. Every name, port, URL, user and password is a
`FS_MSGTEST_*` variable there, and that file is the only place they are defined.

## Ports

| Service | Host address | Notes |
| --- | --- | --- |
| SMTP submission | 127.0.0.1:18465 | implicit TLS, self-signed |
| IMAP | 127.0.0.1:18993 | implicit TLS, self-signed |
| JMAP | http://127.0.0.1:18480/jmap/session | plain HTTP, also the management API |
| JMAP over HTTPS | https://127.0.0.1:18443/jmap/session | self-signed |
| Matrix client API | http://127.0.0.1:18008 | plain HTTP |

## Test users

| Server | User | Password |
| --- | --- | --- |
| Stalwart | alice@msgtest.test | fs-msgtest-alice-Throwaway-7421 |
| Stalwart | bob@msgtest.test | fs-msgtest-bob-Throwaway-7421 |
| Stalwart management | admin | fs-msgtest-throwaway-admin |
| tuwunel | @alice:msgtest.test | fs-msgtest-alice-Throwaway-7421 |
| tuwunel | @bob:msgtest.test | fs-msgtest-bob-Throwaway-7421 |

Matrix registration token: `fs-msgtest-throwaway-registration-token`.
It is used with the `m.login.registration_token` flow of `/register`.

## Versions

| Software | Release | Image digest | Image revision = release tag commit |
| --- | --- | --- | --- |
| Stalwart | v1.0.3 | `docker.io/stalwartlabs/stalwart@sha256:73c684a84c24f5b4206ec266a8deb8986fe44a9f623997e07f084388d943c8bd` | `8b32686532d34623e9ce8e1237cf2bed9554af38` |
| tuwunel | v1.9.4 | `docker.io/jevolk/tuwunel@sha256:0e86c6164d6c9f5b60291938eb61ad8395021c37fe58d47a963ea7edddbedaec` | `f473d9a4a8342514302884041e0ccfea1ad503fc` |

GHCR serves the same digests. The digests are of multi-architecture image indexes.
The Stalwart image's version label says only `v1.0`, so its revision label was
compared with the commit of tag v1.0.3, and the running server reports 1.0.3.

## Limits a test must know about

- **Not fully reproducible.** The images are pinned, but Stalwart downloads unpinned
  data at startup, so it needs outbound HTTPS: the WebUI (`webui/releases/download/v2`),
  spam filter rules and ASN/Geo data (`ip-location-db/releases/download/latest`).
  This can change spam filter results over time. Container egress is not blocked.
- **Implicit TLS only.** Stalwart's default listeners are 465 and 993. There is no
  587 or 143, so STARTTLS is not covered.
- **Self-signed certificates.** Stalwart has no trusted certificate, so TLS clients
  must skip verification or pin the certificate. ACME is off.
- **No DKIM keys.** Generation is off, and mail is only delivered locally. Port 25 is
  not published, so inbound MTA-to-MTA delivery is not covered.
- **Encryption is not exercised.** The room has a real `m.room.encryption` state
  event, but the `m.room.encrypted` event carries a placeholder. This proves event
  transport, not Olm or Megolm.
- **Matrix federation is off**, and tuwunel's login rate limits are off so that
  repeated test runs do not hit `M_LIMIT_EXCEEDED`. Rate limiting is not covered.
- **Stalwart auto-ban.** By default an address is banned after 100 failed logins per
  day. Tests of wrong passwords can trigger it, and `down.sh` then `up.sh` clears it.

## Configuration differences from upstream defaults

The images are unmodified and run their normal entrypoint. Their `HEALTHCHECK` is
off so that nothing else runs inside them.

- Stalwart: bootstrapped through the `x:Bootstrap/set` API with ACME and DKIM off and
  logs on stdout. `STALWART_RECOVERY_ADMIN` stays set as the admin credential, which
  is suitable for tests only. `STALWART_PUBLIC_URL` makes the JMAP session document
  advertise the reachable loopback URL.
- Stalwart: `net.ipv4.ip_unprivileged_port_start=0` inside the container, because
  `no-new-privileges` disables the file capabilities the image uses to bind low ports.
- tuwunel: `rocksdb_allow_fallocate=false` because of btrfs, as the server recommends,
  and `grant_admin_to_first_user=false` so both test users are ordinary users.
- Both: `no-new-privileges`, 2 GiB memory limit, 4096 PID limit.
