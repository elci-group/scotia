# Scotia Windows installer

`scotia.nsi` builds `Scotia-Setup.exe`. It is **per-user by default** and that is
the supported configuration.

## Why per-user is the default

The Scotia daemon is a per-user process. A system-wide daemon is explicitly
unsupported (the Linux installer refuses to run as root for the same reason), so
the Windows installer:

- sets `RequestExecutionLevel user` (no UAC prompt on the default path),
- installs under `%LOCALAPPDATA%\Scotia`,
- writes `PATH` and the autostart entry under `HKCU`.

This keeps Scotia inside the user's trust boundary and avoids any
machine-wide attack surface.

## All-users (Administrator) install — optional, managed environments only

The scope page offers an "Install for all users (requires Administrator)" radio
that switches the install dir to `%PROGRAMFILES64%\Scotia` and writes `PATH`
under `HKLM`. It is intended for managed/enterprise images, not everyday use.

To use it you must run the installer elevated:

```powershell
# From an elevated ("Run as administrator") prompt:
.\Scotia-Setup.exe
```

Without elevation the `HKLM` / `%PROGRAMFILES64%` writes fail; the installer
does not silently fall back to per-user. For a dedicated admin build, change
`RequestExecutionLevel user` to `RequestExecutionLevel admin` in `scotia.nsi`
and rebuild.

## Building

Stage release binaries next to the script, then compile:

```powershell
mkdir bin
copy ..\..\target\release\scotia.exe      bin\
copy ..\..\target\release\scotiad.exe     bin\
copy ..\..\target\release\scotia-shim.exe bin\
makensis scotia.nsi
```

The CI `Installer verification` workflow validates that this script keeps its
per-user default and scope controls; the GUI scope page and the elevated
all-users path are covered by the manual checklist in
`docs/installer-verification.md`.
