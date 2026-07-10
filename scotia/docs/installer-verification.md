# Installer verification

The `Installer verification` workflow (`.github/workflows/installers.yml`) runs
headless, structural checks on every platform: the scripts parse, the expected
safety controls are present, and the Linux installer fails closed when it cannot
reach an authenticated manifest. Those checks are cheap and run on every PR that
touches an installer.

They deliberately do **not** cover the parts that need real secrets, proprietary
tooling, or a GUI. This runbook closes that gap before each release.

## Linux — `install-scotia.sh`

- [ ] A release signed with the key pinned in `MINISIGN_PUBKEY` installs cleanly
      on a fresh user account with no `gpg`/`minisign` customisation.
- [ ] Removing `SHA256SUMS.minisig` (and `SHA256SUMS.sig`) makes the installer
      abort with "Refusing to install an unauthenticated binary."
- [ ] Tampering with one byte of the asset makes the installer abort on the
      checksum step.
- [ ] `--insecure-allow-unsigned` succeeds against an unsigned manifest and
      prints the unsafe-warning.
- [ ] The script refuses `http://` base URLs (curl `--proto '=https'`).

## macOS — `installer/macos/build-pkg.sh`

Needs a macOS host with Xcode command-line tools and built release binaries in
`target/release/`.

- [ ] `cargo build --release --bins` then `./installer/macos/build-pkg.sh`
      produces `scotia.pkg` and a DMG.
- [ ] The resulting `.pkg` installs `scotia`, `scotiad`, and `scotia-shim` under
      `/usr/local/scotia/bin`.
- [ ] `scotia daemon install-service` (run as the user, not root) loads the
      launchd agent and `scotia status` reaches the daemon.
- [ ] For distribution: notarize the DMG (`xcrun notarytool submit`) and staple
      (`xcrun stapler staple`). Notarization requires an Apple Developer ID and
      app-specific password and is not covered by CI.

## Windows — `installer/windows/scotia.nsi`

Needs NSIS (`makensis`) and release binaries staged in `bin/` next to the
script.

- [ ] `makensis scotia.nsi` produces `Scotia-Setup.exe` without warnings.
- [ ] The default scope page selection installs into `%LOCALAPPDATA%\Scotia`
      with `RequestExecutionLevel user` (no UAC prompt).
- [ ] Selecting "all users" triggers the Administrator path (see
      `installer/windows/README.md`) and elevation prompt.
- [ ] Shims are created on the user PATH and `claude`/`codex` resolve through
      the shim to the real binary.

## Release gate

All three platforms must pass their CI job and this manual checklist before a
tag is published through `.github/workflows/release-sign.yml`.
