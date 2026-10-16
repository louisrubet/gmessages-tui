//! Reset only the local login, preserving all other application state.

use std::io;
use std::path::Path;

/// Forget the local session, but do not revoke the device on Google's servers.
pub fn reset(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_is_idempotent_preserves_other_files_and_reports_errors() {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "gmessages-reset-test-{:x}",
            u64::from_ne_bytes(random)
        ));
        std::fs::create_dir(&dir).unwrap();
        let session = dir.join("session.json");
        let marks = dir.join("read-marks.json");
        std::fs::write(&session, b"{}").unwrap();
        std::fs::write(&marks, b"preserve").unwrap();
        reset(&session).unwrap();
        assert!(!session.exists());
        assert_eq!(std::fs::read(&marks).unwrap(), b"preserve");
        reset(&session).unwrap();
        std::fs::create_dir(&session).unwrap();
        assert!(
            reset(&session).is_err(),
            "unexpected errors must not be swallowed"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
