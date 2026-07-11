# Release signing

Scotia's published Linux asset (`scotia-linux-x64`) is authenticated end to end.
The installer (`scotia.tech/downloads/install-scotia.sh`) refuses to run an
unauthenticated binary by default.

## Chain of trust

1. The release workflow (`.github/workflows/release-sign.yml`) builds the asset,
   writes `SHA256SUMS`, and uploads both to the release. When the
   `MINISIGN_SECRET_KEY` repository secret is present it then signs the manifest
   with minisign (`SHA256SUMS.minisig`) and uploads the signature; when the
   secret is absent the run warns and ships the release unsigned — a missing key
   never blocks a release. An existing release can be (re)signed without a new
   tag via the workflow's manual dispatch (Actions → Release signing → Run
   workflow, passing the tag).
2. The installer downloads the asset, `SHA256SUMS`, and `SHA256SUMS.minisig`.
3. It verifies the SHA-256 of the asset against the manifest (integrity), then
   verifies the minisign signature over the manifest against a **pinned public
   key** compiled into the installer (`MINISIGN_PUBKEY`).

The checksum proves the bytes were not corrupted; the signature over the
checksum file proves the manifest itself was produced by the release key. A
network attacker who swaps the asset but cannot produce a valid minisign
signature over the new checksum is rejected.

## Current status: unsigned until armed

Signing is **not yet armed**: `MINISIGN_SECRET_KEY` is not provisioned and the
installer still ships the placeholder `MINISIGN_PUBKEY`, so releases (including
v0.1.6) are published unsigned. The installer therefore aborts with "Refusing
to install an unauthenticated binary" on every release by default — that is the
fail-closed design working as intended, not a bug. For evaluation installs,
pass `--insecure-allow-unsigned` (checksum verification still runs).

To arm signing, follow "Key management" below; the next published release then
ships signed, and already-published releases can be re-signed in place via the
manual workflow dispatch described above.

## Key management

Generate a keypair once, offline:

```sh
minisign -G -p minisign.pub -s minisign.key
```

- `minisign.pub` → paste the single-line key into `MINISIGN_PUBKEY` in
  `scotia.tech/downloads/install-scotia.sh` (replacing the
  `REPLACE_ME_WITH_RELEASE_MINISIGN_PUBLIC_KEY` placeholder).
- `minisign.key` → store as the GitHub repository secret `MINISIGN_SECRET_KEY`
  for `.github/workflows/release-sign.yml`. Keep an offline backup.

The pinned-key approach needs no keyring and no trust-on-first-use: the
installer already knows the only key it will accept.

### Rotation

1. Generate a new keypair.
2. Update `MINISIGN_PUBKEY` in the installer and ship a new installer revision.
3. Replace the `MINISIGN_SECRET_KEY` repository secret.
4. Publish the next release — old assets remain verifiable with the installer
   revision that pinned the old key.

## Fallbacks and escape hatch

- If `minisign` is not installed locally, the installer falls back to a gpg
  detached signature (`SHA256SUMS.sig`) verified against the user's keyring.
- `--insecure-allow-unsigned` skips signature enforcement and proceeds on the
  checksum only. This exists for local development and air-gapped testing; it
  disables authenticity verification and must never be used for production
  installs. The installer prints a loud warning when it is used.

A missing signature, a bad signature, or a placeholder `MINISIGN_PUBKEY` all
fail closed (abort) unless the escape hatch is explicitly passed.
