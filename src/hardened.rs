//! Hardened inside-root file reads, shared by skills and custom tools.
//!
//! One disciplined reader: `symlink_metadata` classifies FIFOs, sockets, and
//! symlinks without opening anything that could block; `canonicalize`
//! refuses escapes; the file opens with `O_NONBLOCK | O_NOFOLLOW` on Unix and
//! the opened handle is re-validated (`is_file`, plus `dev`/`ino` on Unix)
//! so a race-swap is caught; containment is re-checked after open; the body
//! is capped at `max_bytes + 1`.

use std::path::{Path, PathBuf};

/// Open for reading without blocking on FIFOs and without following a
/// final-component symlink (CWE-367). On non-Unix, plain open.
/// Platform-provided flags: hand-rolled octal constants are wrong on some
/// targets (`0o2000` is `O_APPEND`, not `O_NONBLOCK`, on Linux).
#[cfg(unix)]
pub(crate) fn open_nofollow_nonblock(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
pub(crate) fn open_nofollow_nonblock(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Canonicalize `root` when it exists, else absolutize it.
pub(crate) fn canonical_root(root: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(root) {
        return canon;
    }
    if root.is_absolute() {
        return root.to_path_buf();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(root)
}

fn refused(path: &Path, noun: &str) -> String {
    format!("{} escapes the {} directory", path.display(), noun)
}

fn unreadable(path: &Path, err: &dyn std::fmt::Display) -> String {
    format!("could not read {}: {err:#}", path.display())
}

/// Verify every ancestor of `canon` up to (not including) `canon_root` is a
/// non-symlink directory, using `symlink_metadata` only (no opens, no follows).
fn verify_parent_components(
    canon: &Path,
    canon_root: &Path,
    orig: &Path,
    noun: &str,
) -> Result<(), String> {
    let mut cur = canon.parent();
    while let Some(dir) = cur {
        if dir == canon_root || canon_root.starts_with(dir) {
            break;
        }
        if !dir.starts_with(canon_root) {
            return Err(refused(orig, noun));
        }
        match std::fs::symlink_metadata(dir) {
            Ok(m) => {
                if m.file_type().is_symlink() {
                    return Err(format!("{} changed during read; refusing", orig.display()));
                }
                if !m.file_type().is_dir() {
                    return Err(format!("{} is not a regular file", orig.display()));
                }
            }
            Err(e) => return Err(unreadable(orig, &e)),
        }
        cur = dir.parent();
    }
    Ok(())
}

/// Read one file inside `root`, refusing escapes, non-regular files, and
/// files over `max_bytes`. `noun` (`"skills"` / `"tools"`) names the scope
/// in escape errors. Errors are plain messages; callers wrap them in their
/// own error type.
pub(crate) fn read_inside_bytes(
    root: &Path,
    path: &Path,
    max_bytes: u64,
    noun: &str,
) -> Result<Vec<u8>, String> {
    let canon_root = canonical_root(root);
    let meta = std::fs::symlink_metadata(path).map_err(|e| unreadable(path, &e))?;
    if meta.len() > max_bytes {
        return Err(format!(
            "{} is larger than {} bytes",
            path.display(),
            max_bytes
        ));
    }
    let canon = std::fs::canonicalize(path).map_err(|e| unreadable(path, &e))?;
    if !canon.starts_with(&canon_root) {
        return Err(refused(path, noun));
    }
    verify_parent_components(&canon, &canon_root, path, noun)?;
    let canon_meta = std::fs::metadata(&canon).map_err(|e| unreadable(path, &e))?;
    if !canon_meta.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let file = open_nofollow_nonblock(&canon).map_err(|e| unreadable(path, &e))?;
    let opened_meta = file.metadata().map_err(|e| unreadable(path, &e))?;
    if !opened_meta.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if canon_meta.dev() != opened_meta.dev() || canon_meta.ino() != opened_meta.ino() {
            return Err(format!("{} changed during read; refusing", path.display()));
        }
    }
    let recanon = std::fs::canonicalize(path).map_err(|e| unreadable(path, &e))?;
    if !recanon.starts_with(&canon_root) {
        return Err(refused(path, noun));
    }
    verify_parent_components(&recanon, &canon_root, path, noun)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Fail closed: if the re-check cannot even stat the path, containment
        // can no longer be attested, so refuse rather than trusting the
        // earlier checks.
        let re_meta = std::fs::metadata(&recanon).map_err(|e| unreadable(path, &e))?;
        if re_meta.dev() != opened_meta.dev() || re_meta.ino() != opened_meta.ino() {
            return Err(format!("{} changed during read; refusing", path.display()));
        }
    }
    let mut buf = Vec::new();
    {
        use std::io::Read;
        file.take(max_bytes + 1)
            .read_to_end(&mut buf)
            .map_err(|e| unreadable(path, &e))?;
    }
    if buf.len() as u64 > max_bytes {
        return Err(format!(
            "{} is larger than {} bytes",
            path.display(),
            max_bytes
        ));
    }
    Ok(buf)
}
