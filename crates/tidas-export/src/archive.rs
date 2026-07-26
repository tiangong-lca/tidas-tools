use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::Builder;
use tidas_runtime::{CancellationToken, MemoryBudget, RuntimeError};
use walkdir::WalkDir;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::{ExportError, validate_relative_path};

const COPY_BUFFER_BYTES: usize = 1024 * 1024;

pub struct ArchiveSummary {
    pub members: u64,
    pub bytes: u64,
    pub sha256: String,
}

pub fn write_deterministic_zip(
    package_dir: &Path,
    output_zip: &Path,
    cancellation: &CancellationToken,
    memory_budget: &MemoryBudget,
) -> Result<ArchiveSummary, ExportError> {
    cancellation.check()?;
    let parent = output_zip.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = Builder::new()
        .prefix(".tidas-export-archive-")
        .suffix(".zip")
        .tempfile_in(parent)?;
    let writer_file = temporary.reopen()?;
    let mut writer = zip::ZipWriter::new(writer_file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6))
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    let directory_options = options.unix_permissions(0o755);

    let mut entries = collect_entries(package_dir)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut members = 0_u64;
    let reservation = memory_budget
        .reserve(u64::try_from(COPY_BUFFER_BYTES).map_err(|_| RuntimeError::SizeOverflow)?)?;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    for (name, path, is_directory) in entries {
        cancellation.check()?;
        if is_directory {
            writer.add_directory(format!("{name}/"), directory_options)?;
        } else {
            writer.start_file(name, options)?;
            let mut source = File::open(path)?;
            loop {
                cancellation.check()?;
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                writer.write_all(&buffer[..read])?;
            }
        }
        members += 1;
    }
    drop(reservation);
    let finished = writer.finish()?;
    finished.sync_all()?;
    drop(finished);
    let (_, temporary_path) = temporary.keep().map_err(|error| error.error)?;
    publish_atomically(&temporary_path, output_zip)?;

    let metadata = fs::metadata(output_zip)?;
    let sha256 = hash_file(output_zip, cancellation, memory_budget)?;
    Ok(ArchiveSummary {
        members,
        bytes: metadata.len(),
        sha256,
    })
}

fn collect_entries(package_dir: &Path) -> Result<Vec<(String, PathBuf, bool)>, ExportError> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(package_dir).follow_links(false) {
        let entry = entry.map_err(|error| {
            ExportError::Io(
                error
                    .into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("failed to walk export package")),
            )
        })?;
        if entry.path() == package_dir {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Err(ExportError::UnsafePath(entry.path().to_path_buf()));
        }
        let relative = entry
            .path()
            .strip_prefix(package_dir)
            .map_err(|_| ExportError::UnsafePath(entry.path().to_path_buf()))?;
        validate_relative_path(relative)?;
        let name = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        entries.push((name, entry.path().to_path_buf(), entry.file_type().is_dir()));
    }
    Ok(entries)
}

fn publish_atomically(staged: &Path, target: &Path) -> Result<(), ExportError> {
    if !target.exists() {
        return fs::rename(staged, target).map_err(ExportError::Io);
    }
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let backup_dir = Builder::new()
        .prefix(".tidas-export-backup-")
        .tempdir_in(parent)?;
    let backup = backup_dir.path().join("previous.zip");
    fs::rename(target, &backup)?;
    if let Err(source) = fs::rename(staged, target) {
        return match fs::rename(&backup, target) {
            Ok(()) => Err(ExportError::Io(source)),
            Err(restore) => {
                let _preserved_backup = backup_dir.keep();
                Err(ExportError::CommitRollback { source, restore })
            }
        };
    }
    Ok(())
}

fn hash_file(
    path: &Path,
    cancellation: &CancellationToken,
    memory_budget: &MemoryBudget,
) -> Result<String, ExportError> {
    let reservation = memory_budget
        .reserve(u64::try_from(COPY_BUFFER_BYTES).map_err(|_| RuntimeError::SizeOverflow)?)?;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    loop {
        cancellation.check()?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    drop(reservation);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_bytes_are_repeatable_and_replace_existing_output_atomically() {
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path().join("package");
        fs::create_dir_all(package.join("flows")).unwrap();
        fs::write(package.join("flows/b.json"), b"{\"b\":2}\n").unwrap();
        fs::write(package.join("flows/a.json"), b"{\"a\":1}\n").unwrap();
        let first = temporary.path().join("first.zip");
        let second = temporary.path().join("second.zip");
        let cancellation = CancellationToken::default();
        let budget = MemoryBudget::new(8 * 1024 * 1024);
        let first_summary =
            write_deterministic_zip(&package, &first, &cancellation, &budget).unwrap();
        let second_summary =
            write_deterministic_zip(&package, &second, &cancellation, &budget).unwrap();
        assert_eq!(first_summary.sha256, second_summary.sha256);
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());

        fs::write(&second, b"previous archive").unwrap();
        let replacement =
            write_deterministic_zip(&package, &second, &cancellation, &budget).unwrap();
        assert_eq!(replacement.sha256, first_summary.sha256);
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    }

    #[test]
    fn cancellation_never_publishes_an_archive() {
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path().join("package");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("record.json"), b"{}\n").unwrap();
        let output = temporary.path().join("cancelled.zip");
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            write_deterministic_zip(
                &package,
                &output,
                &cancellation,
                &MemoryBudget::new(8 * 1024 * 1024)
            ),
            Err(ExportError::Runtime(RuntimeError::Cancelled))
        ));
        assert!(!output.exists());
    }
}
