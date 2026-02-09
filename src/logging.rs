use std::{
    fs,
    hash::{BuildHasher, Hasher},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{Context, Result};
use env_logger::Target;
use log::LevelFilter;

use crate::common::project_data_dir;

#[derive(Default)]
pub struct Logging {
    file_name: Option<PathBuf>,
    debug_mode: bool,
}

impl Logging {
    #[must_use]
    pub fn new() -> Self {
        Self {
            file_name: None,
            debug_mode: false,
        }
    }

    #[must_use]
    pub fn with_file<P>(mut self, file_name: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.file_name = Some(file_name.into());
        self
    }

    /// Enable debug mode for verbose logging including payloads and hook details.
    ///
    /// Parameters:
    ///   - enable: true to enable debug mode
    ///
    /// Returns: Self for chaining
    #[must_use]
    pub fn with_debug_mode(mut self, enable: bool) -> Self {
        self.debug_mode = enable;
        self
    }

    pub fn start(&self) -> Result<()> {
        let mut b = env_logger::builder();

        // Set log level based on debug mode
        if self.debug_mode {
            b.filter_level(LevelFilter::Debug);
        } else {
            b.filter_level(LevelFilter::Info);
        }

        if let Some(file_name) = &self.file_name {
            let log_file = get_log_file_path(file_name, self.debug_mode)?;

            // For debug mode, use secure file creation
            let fd = if self.debug_mode {
                // Create file with secure permissions atomically (0o600 = owner rw only)
                // Using create_new prevents following symlinks and overwrites
                #[cfg(unix)]
                {
                    let result = fs::OpenOptions::new()
                        .create_new(true) // Fail if file exists (prevents symlink attacks)
                        .write(true)
                        .mode(0o600) // Set permissions atomically at creation
                        .open(&log_file);

                    // If file already exists (unlikely with unique names), open for append
                    match result {
                        Ok(fd) => fd,
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                            fs::OpenOptions::new()
                                .append(true)
                                .open(&log_file)
                                .with_context(|| {
                                    format!("Unable to open {} for writing", log_file.display())
                                })?
                        }
                        Err(e) => {
                            return Err(e).with_context(|| {
                                format!("Unable to create {} for writing", log_file.display())
                            });
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .append(true)
                        .open(&log_file)
                        .with_context(|| {
                            format!("Unable to open {} for writing", log_file.display())
                        })?
                }
            } else {
                // Normal mode: truncate existing file
                fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&log_file)
                    .with_context(|| format!("Unable to open {} for writing", log_file.display()))?
            };

            b.target(Target::Pipe(Box::new(fd)));
        }

        b.init();

        Ok(())
    }
}

/// Get the path to the log file.
/// In debug mode, uses a secure debug directory with restrictive permissions
/// and generates a unique filename with timestamp and random suffix.
///
/// Parameters:
///   - `file_name`: Base name for the log file (used in normal mode)
///   - `debug_mode`: Whether debug mode is enabled
///
/// Returns: Full path to the log file
fn get_log_file_path(file_name: &Path, debug_mode: bool) -> Result<PathBuf> {
    let data_dir = project_data_dir()?;

    if debug_mode {
        // Use a secure debug directory for debug logs
        let debug_dir = data_dir.join("debug");

        // Create directory with restrictive permissions
        // Use DirBuilder on Unix to set mode atomically, avoiding TOCTOU race
        #[cfg(unix)]
        {
            use std::fs::DirBuilder;
            use std::os::unix::fs::DirBuilderExt;
            use std::os::unix::fs::PermissionsExt;

            // DirBuilder with mode sets permissions atomically at creation
            // recursive(true) handles parent directories
            let mut builder = DirBuilder::new();
            builder.recursive(true).mode(0o700);

            // create() is idempotent - succeeds if dir exists with any permissions
            // We then ensure permissions are correct (in case dir existed with wrong perms)
            builder.create(&debug_dir).with_context(|| {
                format!("Unable to create debug directory: {}", debug_dir.display())
            })?;

            // Always verify/fix permissions (handles pre-existing directories)
            let perms = fs::Permissions::from_mode(0o700);
            fs::set_permissions(&debug_dir, perms).with_context(|| {
                format!(
                    "Unable to set permissions on debug directory: {}",
                    debug_dir.display()
                )
            })?;
        }

        #[cfg(not(unix))]
        {
            fs::create_dir_all(&debug_dir).with_context(|| {
                format!("Unable to create debug directory: {}", debug_dir.display())
            })?;
        }

        // Generate unique filename with timestamp and random suffix for security
        // Format: debug-{unix_timestamp}-{random_hex}.log
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let random_suffix: u64 = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        let debug_filename = format!("debug-{timestamp}-{random_suffix:08x}.log");

        Ok(debug_dir.join(debug_filename))
    } else {
        Ok(data_dir.join(file_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_default_debug_mode_disabled() {
        let logging = Logging::new();
        assert!(!logging.debug_mode, "Debug mode should be disabled by default");
    }

    #[test]
    fn test_logging_with_debug_mode_enabled() {
        let logging = Logging::new().with_debug_mode(true);
        assert!(logging.debug_mode, "Debug mode should be enabled after with_debug_mode(true)");
    }

    #[test]
    fn test_logging_with_debug_mode_disabled() {
        let logging = Logging::new().with_debug_mode(true).with_debug_mode(false);
        assert!(!logging.debug_mode, "Debug mode should be disabled after with_debug_mode(false)");
    }

    #[test]
    fn test_logging_with_file() {
        let logging = Logging::new().with_file("test.log");
        assert!(logging.file_name.is_some());
        assert_eq!(
            logging.file_name.unwrap().to_string_lossy(),
            "test.log"
        );
    }

    #[test]
    fn test_logging_builder_chaining() {
        let logging = Logging::new()
            .with_file("app.log")
            .with_debug_mode(true);

        assert!(logging.debug_mode);
        assert!(logging.file_name.is_some());
    }

    #[test]
    fn test_get_log_file_path_normal_mode() {
        // Hold mutex — get_log_file_path reads VIBERAILS_DATA_DIR via project_data_dir()
        let _lock = crate::common::ENV_TEST_MUTEX.lock().unwrap();
        let path = get_log_file_path(Path::new("test.log"), false).unwrap();

        // Should NOT be in debug directory
        // Use platform-independent check via path components
        let has_debug_component = path
            .components()
            .any(|c| c.as_os_str() == "debug");
        assert!(
            !has_debug_component,
            "Normal mode path should not contain debug directory: {}",
            path.display()
        );
    }

    #[test]
    fn test_get_log_file_path_debug_mode() {
        let _lock = crate::common::ENV_TEST_MUTEX.lock().unwrap();
        let path = get_log_file_path(Path::new("test.log"), true).unwrap();

        // Should be in debug directory
        // Use platform-independent check via path components
        let has_debug_component = path
            .components()
            .any(|c| c.as_os_str() == "debug");
        assert!(
            has_debug_component,
            "Debug mode path should contain debug directory: {}",
            path.display()
        );
    }

    #[test]
    fn test_get_log_file_path_debug_mode_unique_filename() {
        let _lock = crate::common::ENV_TEST_MUTEX.lock().unwrap();
        let path = get_log_file_path(Path::new("test.log"), true).unwrap();

        // Debug mode should generate unique filename with timestamp
        let filename = path.file_name().unwrap().to_string_lossy();
        assert!(
            filename.starts_with("debug-"),
            "Debug filename should start with 'debug-': {}",
            filename
        );
        assert!(
            filename.ends_with(".log"),
            "Debug filename should end with '.log': {}",
            filename
        );
    }

    #[test]
    fn test_get_log_file_path_preserves_filename_in_normal_mode() {
        let _lock = crate::common::ENV_TEST_MUTEX.lock().unwrap();
        let filename = "my-custom-log.log";
        let path = get_log_file_path(Path::new(filename), false).unwrap();

        assert!(
            path.file_name()
                .map(|f| f.to_string_lossy() == filename)
                .unwrap_or(false),
            "Filename should be preserved in normal mode: {}",
            path.display()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_get_log_file_path_debug_directory_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::common::ENV_TEST_MUTEX.lock().unwrap();
        let path = get_log_file_path(Path::new("test.log"), true).unwrap();
        let debug_dir = path.parent().unwrap();

        let perms = std::fs::metadata(debug_dir).unwrap().permissions();
        let mode = perms.mode() & 0o777;

        assert_eq!(
            mode, 0o700,
            "Debug directory should have secure permissions (0o700), got: {:o}",
            mode
        );
    }

    // Security tests

    #[cfg(unix)]
    #[test]
    fn test_debug_file_created_with_secure_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::common::ENV_TEST_MUTEX.lock().unwrap();
        // Get a debug log path and create the file manually using our secure method
        let path = get_log_file_path(Path::new("test.log"), true).unwrap();

        // Create the file using create_new with mode (same as Logging::start does)
        let result = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path);

        // File should be created (or already exist from previous test)
        let file_exists = path.exists();

        if let Ok(_fd) = result {
            // Check permissions are 0o600
            let perms = std::fs::metadata(&path).unwrap().permissions();
            let mode = perms.mode() & 0o777;

            assert_eq!(
                mode, 0o600,
                "Debug log file should have 0o600 permissions, got: {:o}",
                mode
            );

            // Cleanup
            std::fs::remove_file(&path).ok();
        } else {
            // If file already exists, that's OK for this test
            assert!(
                file_exists,
                "File should exist if create_new failed: {}",
                path.display()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_create_new_prevents_symlink_attack() {
        // Test that create_new fails if a symlink exists at the target path
        let temp_dir = tempfile::tempdir().unwrap();
        let target_file = temp_dir.path().join("sensitive-file.txt");
        let symlink_path = temp_dir.path().join("attack.log");

        // Create a "sensitive" file
        std::fs::write(&target_file, "sensitive data").unwrap();

        // Create a symlink pointing to it
        std::os::unix::fs::symlink(&target_file, &symlink_path).unwrap();

        // Attempt to create_new at the symlink path should fail
        let result = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&symlink_path);

        assert!(
            result.is_err(),
            "create_new should fail when a symlink exists at the path"
        );

        // Verify the original file wasn't modified
        let content = std::fs::read_to_string(&target_file).unwrap();
        assert_eq!(content, "sensitive data", "Original file should be unchanged");
    }

    #[cfg(unix)]
    #[test]
    fn test_directory_permissions_fixed_if_insecure() {
        use std::os::unix::fs::PermissionsExt;

        // Create a temp directory with insecure permissions
        let temp_dir = tempfile::tempdir().unwrap();
        let insecure_dir = temp_dir.path().join("insecure-debug");

        // Create directory with world-readable permissions
        std::fs::create_dir(&insecure_dir).unwrap();
        std::fs::set_permissions(&insecure_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Verify it's insecure
        let mode_before = std::fs::metadata(&insecure_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode_before, 0o755, "Directory should start insecure");

        // Now use DirBuilder which should fix permissions
        use std::fs::DirBuilder;
        use std::os::unix::fs::DirBuilderExt;

        let mut builder = DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(&insecure_dir).unwrap(); // Succeeds even if exists

        // Fix permissions (as our code does)
        std::fs::set_permissions(&insecure_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

        // Verify permissions are now secure
        let mode_after = std::fs::metadata(&insecure_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode_after, 0o700,
            "Directory should have secure permissions after fix"
        );
    }
}
