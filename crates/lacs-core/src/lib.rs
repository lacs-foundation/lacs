//! Shared defaults, low-level constants, and configuration loading for the LACS workspace.

pub mod config;

/// Default Unix socket URI used by the daemon in local development.
pub const DEFAULT_LISTEN_URI: &str = "unix:///tmp/lacs-daemon.sock";

/// Default SQLite database path used by the daemon in local development.
pub const DEFAULT_DATABASE_PATH: &str = "/tmp/lacs-daemon.sqlite";

#[cfg(test)]
mod tests {
    use super::{DEFAULT_DATABASE_PATH, DEFAULT_LISTEN_URI};

    #[test]
    fn defaults_are_local_and_absolute() {
        assert!(DEFAULT_LISTEN_URI.starts_with("unix:///"));
        assert!(DEFAULT_DATABASE_PATH.starts_with('/'));
    }
}
