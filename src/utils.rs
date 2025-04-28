/*
Functions for file system operations.
*/

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Verifies if a given path is suitable as a download directory.
///
/// Checks:
/// 1. Path is absolute.
/// 2. Path exists.
/// 3. Path is a directory.
/// 4. Path is writable (basic check).
///
/// Returns `Ok(PathBuf)` with the canonicalized path on success,
/// or `Err(String)` with a descriptive error message on failure.
pub fn verify_download_directory(path_str: &str) -> Result<PathBuf, String> {
    let path = Path::new(path_str);

    // 1. Check if absolute
    if !path.is_absolute() {
        return Err(format!(
            "Path must be absolute, but '{}' is relative.",
            path.display()
        ));
    }

    // Attempt to canonicalize early to resolve symlinks etc.
    let canonical_path = path.canonicalize().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            format!("Path '{}' does not exist.", path.display())
        } else {
            format!(
                "Failed to access path '{}': {}",
                path.display(),
                e
            )
        }
    })?;

    // 2. Check existence (implicitly done by canonicalize)
    // 3. Check if it's a directory
    if !canonical_path.is_dir() {
        return Err(format!(
            "Path '{}' exists but is not a directory.",
            canonical_path.display()
        ));
    }

    // 4. Check writability (basic check)
    // We try creating a temporary file inside the directory.
    let temp_file_name = format!(".swapbytes_write_test_{}", std::process::id());
    let temp_file_path = canonical_path.join(&temp_file_name);

    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_file_path)
    {
        Ok(file) => {
            // Clean up the temporary file immediately
            drop(file); // Close the file handle
            let _ = fs::remove_file(&temp_file_path); // Attempt removal, ignore error if it fails
            Ok(canonical_path)
        }
        Err(e) => Err(format!(
            "Directory '{}' is not writable: {}",
            canonical_path.display(),
            e
        )),
    }
}

/// Verifies if a given nickname is valid according to SwapBytes rules.
///
/// Checks:
/// 1. Length is between 3 and 16 characters (inclusive).
/// 2. Contains only allowed characters: a-z, A-Z, 0-9, -, _
///
/// Returns `Ok(String)` with the validated nickname on success,
/// or `Err(String)` with a descriptive error message on failure.
pub fn verify_nickname(name: &str) -> Result<String, String> {
    // 1. Check length
    if !(3..=16).contains(&name.len()) {
        return Err("Name must be between 3-16 characters.".to_string());
    }

    // 2. Check characters
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("Name must only contain: a-z, A-Z, 0-9, -, and _.".to_string());
    }

    // TODO: Add check for nickname uniqueness (requires network state)

    Ok(name.to_string())
} 