use std::fs;
use std::path::{Path, PathBuf};

use tempfile::{Builder, TempDir};

use crate::ConversionError;

pub struct StagedDirectory {
    target: PathBuf,
    staging: TempDir,
}

impl StagedDirectory {
    pub fn new(target: &Path) -> Result<Self, ConversionError> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let staging = Builder::new()
            .prefix(".tidas-conversion-")
            .tempdir_in(parent)?;
        Ok(Self {
            target: target.to_path_buf(),
            staging,
        })
    }

    pub fn path(&self) -> &Path {
        self.staging.path()
    }

    pub fn commit(self) -> Result<(), ConversionError> {
        let parent = self.target.parent().unwrap_or_else(|| Path::new("."));
        if !self.target.exists() {
            return fs::rename(self.staging.path(), &self.target).map_err(ConversionError::Io);
        }

        let backup = Builder::new()
            .prefix(".tidas-conversion-backup-")
            .tempdir_in(parent)?;
        let previous = backup.path().join("previous");
        fs::rename(&self.target, &previous)?;
        if let Err(source) = fs::rename(self.staging.path(), &self.target) {
            let restore = fs::rename(&previous, &self.target);
            return match restore {
                Ok(()) => Err(ConversionError::Io(source)),
                Err(restore) => Err(ConversionError::CommitRollback { source, restore }),
            };
        }
        Ok(())
    }
}
