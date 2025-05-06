/*
Functions for utility operations.
*/

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// --- libp2p Imports ---
use libp2p::PeerId;

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
/// 3. Must not be "global" or "Global"
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

    // 3. Check for "global" or "Global"
    if name.eq_ignore_ascii_case("global") {
        return Err("Nickname cannot be 'global'.".to_string());
    }

    Ok(name.to_string())
}

/// Verifies if a given path points to a readable file suitable for offering.
///
/// Checks:
/// 1. Path exists.
/// 2. Path points to a file, not a directory or symlink (after resolving).
/// 3. Path is readable (basic check).
///
/// Resolves relative paths against the current working directory.
///
/// Returns `Ok((PathBuf, u64))` with the canonicalized path and file size in bytes on success,
/// or `Err(String)` with a descriptive error message on failure.
pub fn verify_offer_file(path_str: &str) -> Result<(PathBuf, u64), String> {
    let path = Path::new(path_str);

    // Resolve the path (handles relative paths and checks existence)
    let canonical_path = path.canonicalize().map_err(|e| {
        match e.kind() {
            io::ErrorKind::NotFound => format!("File not found: '{}'", path.display()),
            io::ErrorKind::PermissionDenied => format!("Permission denied accessing path for '{}'", path.display()),
            _ => format!("Error accessing path '{}': {}", path.display(), e),
        }
    })?;

    // Check readability (basic check by trying to open)
    match fs::metadata(&canonical_path) { // Get metadata to check size and existence again
        Ok(metadata) => {
            if metadata.is_file() { // Double-check it's still a file
                Ok((canonical_path, metadata.len())) // Return path and size
            } else {
                Err(format!("Path is not a file after resolving: '{}'", canonical_path.display()))
            }
        }
        Err(e) => Err(format!(
            "File is not accessible or readable: '{}' ({})",
            canonical_path.display(),
            e
        )),
    }
}

/// Formats a byte count into a human-readable string with units (Bytes, KB, MB, GB).
///
/// Examples:
/// - `format_bytes(123)` -> "123 Bytes"
/// - `format_bytes(12345)` -> "12.06 KB"
/// - `format_bytes(12345678)` -> "11.77 MB"
pub fn format_bytes(bytes: u64) -> String {
    const UNIT_PREFIXES: [&str; 5] = ["Bytes", "KB", "MB", "GB", "TB"]; // Add more if needed (PB, EB...)
    const FACTOR: f64 = 1024.0;

    if bytes == 0 {
        return "0 Bytes".to_string();
    }

    // Calculate the appropriate unit index
    let i = (bytes as f64).log(FACTOR).floor() as usize;
    let unit = UNIT_PREFIXES.get(i).unwrap_or(&"Bytes"); // Fallback to Bytes if index is out of bounds (very large numbers)

    // Calculate the value in the chosen unit
    let value = bytes as f64 / FACTOR.powi(i as i32);

    // Format the value with appropriate precision
    if i == 0 { // Bytes
        format!("{} Bytes", bytes)
    } else if value < 10.0 {
        format!("{:.2} {}", value, unit)
    } else if value < 100.0 {
        format!("{:.1} {}", value, unit)
    } else {
        format!("{:.0} {}", value, unit)
    }
}

// --- PeerId Utilities ---

/// Converts a PeerId into a short, readable string (e.g., "user(...abcdef)").
pub fn peer_id_to_short_string(peer_id: &PeerId) -> String {
    let id_str = peer_id.to_base58();
    let len = id_str.len();
    if len <= 6 {
        format!("user(...{})", id_str)
    } else {
        format!("user(...{})", &id_str[len - 6..])
    }
}
