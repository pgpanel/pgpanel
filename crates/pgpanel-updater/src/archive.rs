//! Secure tar.gz extraction without path traversal or symlink escape.

use crate::error::{UpdaterError, UpdaterResult};
use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use tar::Archive;

/// Maximum single extracted file size (512 MiB).
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// Maximum total extracted bytes (1 GiB).
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

/// Reject paths that escape the destination via `..`, absolute paths, or prefixes.
pub fn validate_archive_path(path: &Path) -> UpdaterResult<()> {
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(UpdaterError::Archive(format!(
                    "path traversal rejected: {}",
                    path.display()
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(UpdaterError::Archive(format!(
                    "absolute path rejected: {}",
                    path.display()
                )));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

/// Resolve a safe destination path under `dest_root`.
pub fn safe_join(dest_root: &Path, entry_path: &Path) -> UpdaterResult<PathBuf> {
    validate_archive_path(entry_path)?;
    let joined = dest_root.join(entry_path);
    let canonical_root = dest_root
        .canonicalize()
        .unwrap_or_else(|_| dest_root.to_path_buf());
    let parent = joined
        .parent()
        .ok_or_else(|| UpdaterError::Archive("invalid entry path".into()))?;
    fs::create_dir_all(parent)?;
    let canonical_parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(UpdaterError::Archive(format!(
            "resolved path escapes destination: {}",
            joined.display()
        )));
    }
    Ok(joined)
}

/// Extract a `.tar.gz` archive securely into `dest_root`.
pub fn extract_tar_gz(archive_path: &Path, dest_root: &Path) -> UpdaterResult<()> {
    fs::create_dir_all(dest_root)?;
    let file = File::open(archive_path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut total_bytes: u64 = 0;

    for entry in archive
        .entries()
        .map_err(|e| UpdaterError::Archive(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| UpdaterError::Archive(e.to_string()))?;
        let entry_path = entry
            .path()
            .map_err(|e| UpdaterError::Archive(e.to_string()))?
            .into_owned();

        validate_archive_path(&entry_path)?;

        let header = entry.header().clone();
        let entry_type = header.entry_type();

        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(UpdaterError::Archive(format!(
                "symlink/hardlink entries are not allowed: {}",
                entry_path.display()
            )));
        }

        let size = header
            .size()
            .map_err(|e| UpdaterError::Archive(e.to_string()))?;
        if size > MAX_FILE_BYTES {
            return Err(UpdaterError::Archive(format!(
                "file too large in archive: {} ({} bytes)",
                entry_path.display(),
                size
            )));
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(UpdaterError::Archive(
                "total extracted size exceeds limit".into(),
            ));
        }

        let dest_path = safe_join(dest_root, &entry_path)?;

        if entry_type.is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else if entry_type.is_file() {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            entry
                .unpack(&dest_path)
                .map_err(|e| UpdaterError::Archive(e.to_string()))?;
        } else if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(UpdaterError::Archive(format!(
                "unsupported archive entry type for {}",
                entry_path.display()
            )));
        }
    }

    Ok(())
}

/// Set secure permissions on an extracted release directory.
pub fn set_release_permissions(release_dir: &Path) -> UpdaterResult<()> {
    set_dir_permissions(release_dir, 0o755)?;
    for entry in fs::read_dir(release_dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            set_dir_permissions(&path, 0o755)?;
            if path.file_name().and_then(|n| n.to_str()) == Some("bin") {
                set_bin_permissions(&path)?;
            }
        }
    }
    Ok(())
}

fn set_dir_permissions(path: &Path, mode: u32) -> UpdaterResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(mode);
        fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

fn set_bin_permissions(bin_dir: &Path) -> UpdaterResult<()> {
    for entry in fs::read_dir(bin_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&path, perms)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use tar::{Builder, Header};
    use tempfile::TempDir;

    fn build_archive<F>(write_entries: F) -> tempfile::NamedTempFile
    where
        F: FnOnce(&mut Builder<GzEncoder<Vec<u8>>>),
    {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let enc = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(enc);
        write_entries(&mut builder);
        let inner = builder.into_inner().unwrap().finish().unwrap();
        file.write_all(&inner).unwrap();
        file
    }

    #[test]
    fn validate_rejects_parent_dir() {
        let err = validate_archive_path(Path::new("../etc/passwd")).unwrap_err();
        assert!(matches!(err, UpdaterError::Archive(_)));
    }

    #[test]
    fn validate_rejects_absolute() {
        let err = validate_archive_path(Path::new("/etc/passwd")).unwrap_err();
        assert!(matches!(err, UpdaterError::Archive(_)));
    }

    #[test]
    fn safe_join_rejects_nested_traversal() {
        let dest = TempDir::new().unwrap();
        let err = safe_join(dest.path(), Path::new("foo/../../escape.txt")).unwrap_err();
        assert!(matches!(err, UpdaterError::Archive(_)));
    }

    #[test]
    fn extract_rejects_symlink() {
        let archive = build_archive(|builder| {
            let mut header = Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(15);
            header.set_cksum();
            builder
                .append_data(&mut header, "link", &b"/etc/passwd"[..])
                .unwrap();
        });

        let dest = TempDir::new().unwrap();
        let err = extract_tar_gz(archive.path(), dest.path()).unwrap_err();
        assert!(matches!(err, UpdaterError::Archive(_)));
        let msg = err.to_string();
        assert!(msg.contains("symlink") || msg.contains("hardlink"));
    }

    #[test]
    fn extract_allows_normal_files() {
        let archive = build_archive(|builder| {
            let mut header = Header::new_gnu();
            header.set_size(7);
            header.set_cksum();
            builder
                .append_data(&mut header, "bin/pgpanel-web", &b"#!/bin"[..])
                .unwrap();
            let mut header2 = Header::new_gnu();
            header2.set_entry_type(tar::EntryType::Directory);
            header2.set_size(0);
            header2.set_mode(0o755);
            header2.set_cksum();
            builder.append_data(&mut header2, "bin", &[][..]).unwrap();
        });

        let dest = TempDir::new().unwrap();
        extract_tar_gz(archive.path(), dest.path()).unwrap();
        assert!(dest.path().join("bin/pgpanel-web").is_file());
    }
}
