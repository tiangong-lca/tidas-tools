use std::fs;
use std::path::{Path, PathBuf};

use tempfile::{Builder, TempDir};

use crate::ReleaseError;

pub struct StagedDirectory {
    target: PathBuf,
    staging: TempDir,
}

impl StagedDirectory {
    pub fn new(target: &Path) -> Result<Self, ReleaseError> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let staging = Builder::new()
            .prefix(".tidas-release-")
            .tempdir_in(parent)?;
        Ok(Self {
            target: target.to_path_buf(),
            staging,
        })
    }

    pub fn path(&self) -> &Path {
        self.staging.path()
    }

    pub fn commit(self) -> Result<(), ReleaseError> {
        let parent = self.target.parent().unwrap_or_else(|| Path::new("."));
        if !self.target.exists() {
            return fs::rename(self.staging.path(), &self.target).map_err(ReleaseError::Io);
        }
        let metadata = fs::symlink_metadata(&self.target)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ReleaseError::OutputNotDirectory(self.target));
        }
        let backup = Builder::new()
            .prefix(".tidas-release-backup-")
            .tempdir_in(parent)?;
        let previous = backup.path().join("previous");
        fs::rename(&self.target, &previous)?;
        if let Err(source) = fs::rename(self.staging.path(), &self.target) {
            return match fs::rename(&previous, &self.target) {
                Ok(()) => Err(ReleaseError::Io(source)),
                Err(restore) => {
                    let _preserved_backup = backup.keep();
                    Err(ReleaseError::CommitRollback { source, restore })
                }
            };
        }
        Ok(())
    }
}

pub fn reject_overlapping_output(input: &Path, output: &Path) -> Result<(), ReleaseError> {
    let input = fs::canonicalize(input)?;
    let output = if output.exists() {
        fs::canonicalize(output)?
    } else {
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        fs::canonicalize(parent)?.join(
            output
                .file_name()
                .ok_or_else(|| ReleaseError::OutputInsideInput(output.to_path_buf()))?,
        )
    };
    if output.starts_with(&input) || input.starts_with(&output) {
        Err(ReleaseError::OutputInsideInput(output))
    } else {
        Ok(())
    }
}

pub fn remove_internal_ilcd(ilcd_dir: &Path) -> Result<(), ReleaseError> {
    fs::remove_dir_all(ilcd_dir)?;
    Ok(())
}

pub fn flatten_packages(staging: &Path) -> Result<(), ReleaseError> {
    let packages = staging.join("packages");
    for entry in fs::read_dir(&packages)? {
        let entry = entry?;
        fs::rename(entry.path(), staging.join(entry.file_name()))?;
    }
    fs::remove_dir(packages)?;
    Ok(())
}
