//! Runtime-layout security primitives shared by the daemon (`scotiad`) and the
//! CLI `doctor` checks: the modes the control socket, its parent directory, and
//! the handshake token must carry, plus helpers to create and assert them.
//!
//! Centralising the constants here means the code that *creates* the layout and
//! the code that *verifies* it cannot drift apart.

use std::path::Path;

/// Owner-only directory mode (rwx------) for the runtime directory.
#[cfg(unix)]
pub const PRIVATE_DIR_MODE: u32 = 0o700;
/// Owner-only file mode (rw-------) for the socket node and IPC token.
#[cfg(unix)]
pub const OWNER_ONLY_FILE_MODE: u32 = 0o600;

/// Create a directory (recursively) and lock it to owner-only access.
///
/// Even when the directory already exists we re-assert the mode, so a
/// previously-loose directory cannot be reused to expose the socket. On
/// non-unix targets this degrades to a plain recursive create.
pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(PRIVATE_DIR_MODE);
        builder.create(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_DIR_MODE))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
}

/// Set a file/node to owner-only (0600). No-op on non-unix targets.
pub fn set_owner_only(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(OWNER_ONLY_FILE_MODE))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn ensure_private_dir_creates_owner_only_dir() {
        let dir = std::env::temp_dir().join(format!("scotia-runtime-{}", uuid::Uuid::new_v4()));
        ensure_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        // Re-asserting on an existing dir keeps it private.
        ensure_private_dir(&dir).unwrap();
        let mode2 = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode2, 0o700);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_owner_only_locks_file_to_0600() {
        let f = std::env::temp_dir().join(format!("scotia-runtime-{}", uuid::Uuid::new_v4()));
        std::fs::write(&f, b"x").unwrap();
        set_owner_only(&f).unwrap();
        let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(&f);
    }
}
