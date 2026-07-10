# SPDX-FileCopyrightText: no
# SPDX-License-Identifier: CC0-1.0
#
# Calamares job module for installing Scotia.
#
# Calamares runs as root, typically before the target user's first login, so
# there is no per-user session to register a daemon or PATH shims against.
# Accordingly this module ONLY copies the release binaries into a system-wide
# directory. Per-user wiring (shims + the LaunchAgent/systemd --user service)
# is intentionally deferred: each user runs `scotia install-shims` and/or
# `scotia daemon install-service` after their first login. This keeps the
# daemon a per-user process and avoids installing anything as root against the
# wrong home directory.

import gettext
import os
import shutil

import libcalamares

_ = gettext.translation(
    "calamares-python",
    localedir=libcalamares.utils.gettext_path(),
    languages=libcalamares.utils.gettext_languages(),
    fallback=True,
).gettext


def pretty_name():
    return _("Installing Scotia")


def pretty_status_message():
    return _("Copying Scotia binaries into place...")


def _ensure_dir(path):
    if not os.path.isdir(path):
        os.makedirs(path, mode=0o755, exist_ok=True)


def _copy_binaries(source_dir, bin_dir):
    binaries = ["scotia", "scotia-shim", "scotiad"]
    _ensure_dir(bin_dir)
    copied = []
    for name in binaries:
        src = os.path.join(source_dir, name)
        dst = os.path.join(bin_dir, name)
        if os.path.exists(src):
            shutil.copy2(src, dst)
            os.chmod(dst, 0o755)
            copied.append(name)
        else:
            libcalamares.utils.warning(
                "Scotia binary not found in payload: {}".format(src)
            )
    return copied


def run():
    job_config = libcalamares.job.configuration
    source_dir = job_config.get("binary_source_dir", "/usr/share/scotia/bin")

    # Install location for the binaries. System-wide is the only sensible
    # choice during an OS install; per-user wiring happens post-login.
    bin_dir = job_config.get("binary_install_dir", "/usr/local/bin")

    copied = _copy_binaries(source_dir, bin_dir)
    if not copied:
        return (
            _("Scotia installer failed"),
            _("No Scotia binaries were found in {}".format(source_dir)),
        )

    libcalamares.utils.debug(
        "Scotia binaries installed to {}: {}. Per-user shims and the daemon "
        "service are configured by each user after first login via "
        "`scotia install-shims` / `scotia daemon install-service`.".format(
            bin_dir, ", ".join(copied)
        )
    )
    return None
