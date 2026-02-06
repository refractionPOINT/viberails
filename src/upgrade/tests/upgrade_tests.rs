use std::fs;
use std::time::Duration;

use tempfile::TempDir;

use super::super::poll::*;
use crate::common::PROJECT_NAME;

#[test]
fn test_verify_checksum_valid() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = dir.path().join("test.bin");

    // Write known content
    let content = b"hello world";
    fs::write(&file_path, content).expect("Failed to write test file");

    // SHA256 of "hello world"
    let expected_hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    let result = verify_checksum(&file_path, expected_hash);
    assert!(result.is_ok(), "Checksum verification should succeed");
}

#[test]
fn test_verify_checksum_invalid() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = dir.path().join("test.bin");

    fs::write(&file_path, b"hello world").expect("Failed to write test file");

    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";

    let result = verify_checksum(&file_path, wrong_hash);
    assert!(result.is_err(), "Checksum verification should fail");
}

#[test]
fn test_atomic_replace_binary_new_file() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("source.bin");
    let dst = dir.path().join("dest.bin");

    // Create source file
    fs::write(&src, b"new binary content").expect("Failed to write source");

    // Replace (destination doesn't exist)
    let result = atomic_replace_binary(&src, &dst);
    assert!(
        result.is_ok(),
        "Atomic replace should succeed: {:?}",
        result
    );

    // Verify content
    let content = fs::read_to_string(&dst).expect("Failed to read dest");
    assert_eq!(content, "new binary content");
}

#[test]
fn test_atomic_replace_binary_with_existing() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("source.bin");
    let dst = dir.path().join("dest.bin");

    // Create both files
    fs::write(&src, b"new content").expect("Failed to write source");
    fs::write(&dst, b"old content").expect("Failed to write dest");

    let result = atomic_replace_binary(&src, &dst);
    assert!(
        result.is_ok(),
        "Atomic replace should succeed: {:?}",
        result
    );

    // Verify new content replaced old content
    let content = fs::read_to_string(&dst).expect("Failed to read dest");
    assert_eq!(content, "new content");
}

#[test]
fn test_atomic_replace_binary_cleans_temp_on_failure() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("nonexistent.bin"); // Source doesn't exist
    let dst = dir.path().join("dest.bin");

    // This should fail because source doesn't exist
    let result = atomic_replace_binary(&src, &dst);
    assert!(result.is_err(), "Should fail with nonexistent source");

    // Verify no temp files left behind (they start with .)
    let entries: Vec<_> = fs::read_dir(dir.path())
        .expect("Failed to read dir")
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!(".{PROJECT_NAME}_new_"))
        })
        .collect();
    assert!(
        entries.is_empty(),
        "No temp files should be left on failure"
    );
}

#[test]
fn test_upgrade_binary_path_is_random() {
    // Call twice and verify different paths
    let path1 = upgrade_binary_path().expect("Failed to get upgrade path 1");
    let path2 = upgrade_binary_path().expect("Failed to get upgrade path 2");

    // Paths should be different due to random suffix
    assert_ne!(path1, path2, "Upgrade paths should have random suffixes");

    // Both should contain the upgrade prefix
    let name1 = path1.file_name().unwrap().to_string_lossy();
    let name2 = path2.file_name().unwrap().to_string_lossy();
    assert!(
        name1.contains("_upgrade_"),
        "Path should contain upgrade prefix"
    );
    assert!(
        name2.contains("_upgrade_"),
        "Path should contain upgrade prefix"
    );
}

#[test]
fn test_is_file_older_than_with_new_file() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let test_file = dir.path().join("test.bin");

    // Create a new file
    fs::write(&test_file, b"test").expect("Failed to write test file");

    // File just created should not be older than 1 hour
    assert!(
        !is_file_older_than(&test_file, &Duration::from_secs(3600)),
        "Newly created file should not be older than 1 hour"
    );

    // File just created should not be older than 1 second
    // (avoids flakiness from Duration::ZERO where timestamp resolution matters)
    assert!(
        !is_file_older_than(&test_file, &Duration::from_secs(1)),
        "Newly created file should not be older than 1 second"
    );
}

#[test]
fn test_is_file_older_than_nonexistent() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let nonexistent = dir.path().join("does_not_exist.bin");

    // Nonexistent file should return false
    assert!(
        !is_file_older_than(&nonexistent, &Duration::from_secs(1)),
        "Nonexistent file should return false"
    );
}

#[cfg(unix)]
#[test]
fn test_is_process_running_current() {
    let pid = std::process::id();
    assert!(is_process_running(pid), "Current process should be running");
}

#[cfg(unix)]
#[test]
fn test_is_process_running_nonexistent() {
    // Test with a PID that's extremely unlikely to exist.
    // On Linux, PIDs typically max at 32768 or 4194304 (with pid_max).
    // Use i32::MAX which is a valid pid_t but virtually never exists.
    // Note: This is not a strict guarantee on systems with unusual PID ranges.
    #[allow(clippy::cast_sign_loss)]
    let unlikely_pid = i32::MAX as u32;

    // This PID should not be running on any normal system
    let result = is_process_running(unlikely_pid);

    // Assert the expected behavior - this PID should not exist
    assert!(
        !result,
        "PID {} should not be running on any normal system",
        unlikely_pid
    );
}

#[cfg(unix)]
#[test]
fn test_is_process_running_pid_zero() {
    // PID 0 is the kernel scheduler, kill(0, 0) sends to process group
    // This tests edge case handling
    let result = is_process_running(0);
    // Result depends on permissions, but function should not panic
    let _ = result;
}

#[cfg(windows)]
#[test]
fn test_is_process_running_current_windows() {
    let pid = std::process::id();
    assert!(is_process_running(pid), "Current process should be running");
}

#[cfg(windows)]
#[test]
fn test_is_process_running_nonexistent_windows() {
    // Use a PID that's extremely unlikely to exist on Windows
    // Windows PIDs are typically in the range 0-65535 but can go higher
    // Use u32::MAX which is virtually never a valid PID
    let unlikely_pid = u32::MAX;

    let result = is_process_running(unlikely_pid);
    assert!(
        !result,
        "PID {} should not be running on any normal system",
        unlikely_pid
    );
}

#[test]
fn test_atomic_replace_binary_no_leftover_temp_or_backup() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("source.bin");
    let dst = dir.path().join("dest.bin");

    // Create both source and destination
    fs::write(&src, b"new content").expect("Failed to write source");
    fs::write(&dst, b"old content").expect("Failed to write dest");

    // Perform replacement
    let result = atomic_replace_binary(&src, &dst);
    assert!(result.is_ok(), "Replacement should succeed: {:?}", result);

    // Verify no temp files (.viberails_new_*) or backup files (.viberails_old_*) left
    let leftover_files: Vec<_> = fs::read_dir(dir.path())
        .expect("Failed to read dir")
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with(&format!(".{PROJECT_NAME}_new_"))
                || name.starts_with(&format!(".{PROJECT_NAME}_old_"))
        })
        .collect();

    assert!(
        leftover_files.is_empty(),
        "No temp or backup files should remain after successful replacement, found: {:?}",
        leftover_files
            .iter()
            .map(|e| e.file_name())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_atomic_replace_binary_preserves_dst_on_src_read_failure() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("nonexistent_source.bin");
    let dst = dir.path().join("dest.bin");

    // Create destination with known content
    fs::write(&dst, b"original content").expect("Failed to write dest");

    // Try to replace with nonexistent source - should fail
    let result = atomic_replace_binary(&src, &dst);
    assert!(result.is_err(), "Should fail with nonexistent source");

    // Verify destination is unchanged
    let content = fs::read_to_string(&dst).expect("Failed to read dest");
    assert_eq!(
        content, "original content",
        "Destination should be unchanged after failed replacement"
    );
}

#[cfg(windows)]
#[test]
fn test_atomic_replace_binary_windows_replaces_existing() {
    // Windows-specific test: verify MoveFileExW replaces existing file
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("source.bin");
    let dst = dir.path().join("dest.bin");

    // Create files
    fs::write(&src, b"new content").expect("Failed to write source");
    fs::write(&dst, b"old content").expect("Failed to write dest");

    // Replace using atomic_replace_binary (which uses MoveFileExW on Windows)
    let result = atomic_replace_binary(&src, &dst);
    assert!(result.is_ok(), "Replacement should succeed");

    // Verify content changed
    let content = fs::read_to_string(&dst).expect("Failed to read dest");
    assert_eq!(content, "new content");

    // Verify no leftover temp files
    let leftover_files: Vec<_> = fs::read_dir(dir.path())
        .expect("Failed to read dir")
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with(&format!(".{PROJECT_NAME}_"))
        })
        .collect();

    assert!(
        leftover_files.is_empty(),
        "No temp files should remain after successful replacement"
    );
}

#[cfg(windows)]
#[test]
fn test_move_file_replace_windows_new_file() {
    // Test MoveFileExW when destination doesn't exist
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("source.bin");
    let dst = dir.path().join("dest.bin");

    fs::write(&src, b"test content").expect("Failed to write source");

    let result = move_file_replace_windows(&src, &dst);
    assert!(result.is_ok(), "Move should succeed: {:?}", result);

    // Verify file moved
    assert!(!src.exists(), "Source should no longer exist");
    assert!(dst.exists(), "Destination should exist");
    let content = fs::read_to_string(&dst).expect("Failed to read dest");
    assert_eq!(content, "test content");
}

#[cfg(windows)]
#[test]
fn test_move_file_replace_windows_replaces_existing() {
    // Test MoveFileExW replaces existing destination atomically
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("source.bin");
    let dst = dir.path().join("dest.bin");

    fs::write(&src, b"new content").expect("Failed to write source");
    fs::write(&dst, b"old content").expect("Failed to write dest");

    let result = move_file_replace_windows(&src, &dst);
    assert!(result.is_ok(), "Move should succeed: {:?}", result);

    // Verify replacement
    assert!(!src.exists(), "Source should no longer exist");
    let content = fs::read_to_string(&dst).expect("Failed to read dest");
    assert_eq!(content, "new content");
}

#[cfg(windows)]
#[test]
fn test_move_file_replace_windows_nonexistent_source() {
    // Test MoveFileExW fails gracefully with nonexistent source
    let dir = TempDir::new().expect("Failed to create temp dir");
    let src = dir.path().join("nonexistent.bin");
    let dst = dir.path().join("dest.bin");

    let result = move_file_replace_windows(&src, &dst);
    assert!(result.is_err(), "Move should fail with nonexistent source");
}

#[test]
fn test_get_arch_returns_valid_arch() {
    let arch = get_arch();

    // Should return a non-empty string
    assert!(!arch.is_empty(), "Architecture should not be empty");

    // On known platforms, should return normalized names
    match std::env::consts::ARCH {
        "x86_64" => assert_eq!(arch, "x64"),
        "aarch64" => assert_eq!(arch, "arm64"),
        other => assert_eq!(arch, other),
    }
}
