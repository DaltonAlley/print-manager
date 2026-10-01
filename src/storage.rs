#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    fs,
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};
use uuid::Uuid;

use crate::domain::AttachmentIntegrity;

pub const MAX_LOGO_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Storage {
    data_root: PathBuf,
    max_attachment_bytes: u64,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("the filename is unsafe")]
    UnsafeFilename,
    #[error("the uploaded file exceeds the {limit} byte limit")]
    TooLarge { limit: u64 },
    #[error("the upload did not contain a file")]
    MissingFile,
    #[error("the logo must be a PNG or JPEG no larger than 2000 by 2000 pixels")]
    InvalidLogo,
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct IntegrityReport {
    pub missing: Vec<String>,
    pub corrupt: Vec<String>,
    pub orphaned: Vec<String>,
    pub staging: Vec<String>,
}

impl IntegrityReport {
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty()
            && self.corrupt.is_empty()
            && self.orphaned.is_empty()
            && self.staging.is_empty()
    }
}

pub struct StagedFile {
    file: File,
    temporary_path: PathBuf,
    stored_name: String,
    byte_size: u64,
    limit: u64,
    hasher: Sha256,
}

pub struct StoredFile {
    pub path: PathBuf,
    pub stored_name: String,
    pub byte_size: u64,
    pub sha256: String,
}

pub struct RemovedFile {
    original_path: PathBuf,
    temporary_path: PathBuf,
}

impl Storage {
    pub fn new(data_root: PathBuf, max_attachment_bytes: u64) -> Self {
        Self {
            data_root,
            max_attachment_bytes,
        }
    }

    pub fn max_attachment_bytes(&self) -> u64 {
        self.max_attachment_bytes
    }

    pub fn multipart_body_limit(&self) -> usize {
        usize::try_from(self.max_attachment_bytes.saturating_add(1024 * 1024)).unwrap_or(usize::MAX)
    }

    pub async fn prepare(&self) -> Result<(), StorageError> {
        for directory in [
            self.attachments_dir(),
            self.attachment_staging_dir(),
            self.branding_dir(),
            self.branding_staging_dir(),
        ] {
            fs::create_dir_all(directory).await?;
        }
        Ok(())
    }

    pub async fn begin_attachment(&self) -> Result<StagedFile, StorageError> {
        self.begin(self.attachment_staging_dir(), self.max_attachment_bytes)
            .await
    }

    pub async fn begin_logo(&self) -> Result<StagedFile, StorageError> {
        self.begin(self.branding_staging_dir(), MAX_LOGO_BYTES)
            .await
    }

    async fn begin(&self, staging_dir: PathBuf, limit: u64) -> Result<StagedFile, StorageError> {
        fs::create_dir_all(&staging_dir).await?;
        let stored_name = Uuid::new_v4().to_string();
        let temporary_path = staging_dir.join(format!("{stored_name}.part"));
        let file = File::create(&temporary_path).await?;
        Ok(StagedFile {
            file,
            temporary_path,
            stored_name,
            byte_size: 0,
            limit,
            hasher: Sha256::new(),
        })
    }

    pub async fn commit_attachment(&self, staged: StagedFile) -> Result<StoredFile, StorageError> {
        self.commit(staged, self.attachments_dir()).await
    }

    pub async fn commit_logo(&self, staged: StagedFile) -> Result<StoredFile, StorageError> {
        self.commit(staged, self.branding_dir()).await
    }

    pub async fn validate_logo(&self, stored: &StoredFile) -> Result<(), StorageError> {
        let bytes = fs::read(&stored.path).await?;
        let image = image::load_from_memory(&bytes).map_err(|_| StorageError::InvalidLogo)?;
        if image.width() > 2_000 || image.height() > 2_000 {
            return Err(StorageError::InvalidLogo);
        }
        Ok(())
    }

    async fn commit(
        &self,
        mut staged: StagedFile,
        destination: PathBuf,
    ) -> Result<StoredFile, StorageError> {
        staged.file.flush().await?;
        staged.file.sync_all().await?;
        drop(staged.file);
        fs::create_dir_all(&destination).await?;
        let path = destination.join(&staged.stored_name);
        fs::rename(&staged.temporary_path, &path).await?;
        Ok(StoredFile {
            path,
            stored_name: staged.stored_name,
            byte_size: staged.byte_size,
            sha256: format!("{:x}", staged.hasher.finalize()),
        })
    }

    pub async fn discard(&self, staged: StagedFile) {
        drop(staged.file);
        let _ = fs::remove_file(staged.temporary_path).await;
    }

    pub fn attachment_path(&self, stored_name: &str) -> Result<PathBuf, StorageError> {
        safe_component(stored_name)?;
        Ok(self.attachments_dir().join(stored_name))
    }

    /// Resolves a managed logo identifier strictly within the branding directory.
    pub fn managed_logo_path(&self, identifier: &str) -> Result<PathBuf, StorageError> {
        safe_generated_identifier(identifier)?;
        Ok(self.branding_dir().join(identifier))
    }

    /// Resolves managed identifiers for PDF rendering while retaining legacy path support.
    pub fn logo_path_for_pdf(&self, stored_value: &str) -> PathBuf {
        self.managed_logo_path(stored_value)
            .unwrap_or_else(|_| PathBuf::from(stored_value))
    }

    pub async fn remove_attachment(&self, stored_name: &str) -> Result<(), StorageError> {
        fs::remove_file(self.attachment_path(stored_name)?).await?;
        Ok(())
    }

    pub async fn stage_attachment_removal(
        &self,
        stored_name: &str,
    ) -> Result<RemovedFile, StorageError> {
        let original_path = self.attachment_path(stored_name)?;
        fs::create_dir_all(self.attachment_staging_dir()).await?;
        let temporary_path = self
            .attachment_staging_dir()
            .join(format!("{}.delete", Uuid::new_v4()));
        fs::rename(&original_path, &temporary_path).await?;
        Ok(RemovedFile {
            original_path,
            temporary_path,
        })
    }

    pub async fn restore_attachment(&self, removed: RemovedFile) -> Result<(), StorageError> {
        fs::rename(removed.temporary_path, removed.original_path).await?;
        Ok(())
    }

    pub async fn finish_attachment_removal(
        &self,
        removed: RemovedFile,
    ) -> Result<(), StorageError> {
        fs::remove_file(removed.temporary_path).await?;
        Ok(())
    }

    pub async fn audit(
        &self,
        metadata: &[AttachmentIntegrity],
    ) -> Result<IntegrityReport, StorageError> {
        let expected: HashSet<&str> = metadata
            .iter()
            .map(|item| item.stored_name.as_str())
            .collect();
        let mut report = IntegrityReport::default();
        for item in metadata {
            let path = self.attachment_path(&item.stored_name)?;
            match attachment_integrity(&path, item).await {
                Ok(true) => {}
                Ok(false) => report.corrupt.push(item.stored_name.clone()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    report.missing.push(item.stored_name.clone());
                }
                Err(error) => return Err(error.into()),
            }
        }
        collect_unexpected(&self.attachments_dir(), &expected, &mut report.orphaned).await?;
        collect_names(&self.attachment_staging_dir(), &mut report.staging).await?;
        collect_names(&self.branding_staging_dir(), &mut report.staging).await?;
        report.missing.sort();
        report.corrupt.sort();
        report.orphaned.sort();
        report.staging.sort();
        Ok(report)
    }

    fn attachments_dir(&self) -> PathBuf {
        self.data_root.join("attachments")
    }
    fn attachment_staging_dir(&self) -> PathBuf {
        self.attachments_dir().join(".staging")
    }
    fn branding_dir(&self) -> PathBuf {
        self.data_root.join("branding")
    }
    fn branding_staging_dir(&self) -> PathBuf {
        self.branding_dir().join(".staging")
    }
}

impl StagedFile {
    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        let added = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if self.byte_size.saturating_add(added) > self.limit {
            return Err(StorageError::TooLarge { limit: self.limit });
        }
        self.file.write_all(bytes).await?;
        self.hasher.update(bytes);
        self.byte_size += added;
        Ok(())
    }
}

pub fn validate_original_filename(name: &str) -> Result<(), StorageError> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(StorageError::UnsafeFilename);
    }
    Ok(())
}

fn safe_component(name: &str) -> Result<(), StorageError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(StorageError::UnsafeFilename);
    }
    Ok(())
}

fn safe_generated_identifier(identifier: &str) -> Result<(), StorageError> {
    safe_component(identifier)?;
    let parsed = Uuid::parse_str(identifier).map_err(|_| StorageError::UnsafeFilename)?;
    if parsed.to_string() != identifier {
        return Err(StorageError::UnsafeFilename);
    }
    Ok(())
}

async fn attachment_integrity(
    path: &Path,
    expected: &AttachmentIntegrity,
) -> Result<bool, io::Error> {
    let mut file = File::open(path).await?;
    if file.metadata().await?.len() != u64::try_from(expected.byte_size).unwrap_or(u64::MAX) {
        return Ok(false);
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(&expected.sha256))
}

async fn collect_names(directory: &Path, output: &mut Vec<String>) -> Result<(), io::Error> {
    let mut entries = match fs::read_dir(directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() {
            output.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(())
}

async fn collect_unexpected(
    directory: &Path,
    expected: &HashSet<&str>,
    output: &mut Vec<String>,
) -> Result<(), io::Error> {
    let mut names = Vec::new();
    collect_names(directory, &mut names).await?;
    output.extend(
        names
            .into_iter()
            .filter(|name| !expected.contains(name.as_str())),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_path_separators() {
        for name in ["../secret", "sub/file", "sub\\file", ".", ".."] {
            assert!(
                validate_original_filename(name).is_err(),
                "accepted {name:?}"
            );
        }
        assert!(validate_original_filename("customer art (final).pdf").is_ok());
    }

    #[test]
    fn managed_logos_accept_only_generated_identifiers_beneath_branding() {
        let storage = Storage::new(PathBuf::from("/srv/print-manager"), 4);
        let identifier = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            storage.managed_logo_path(identifier).unwrap_or_default(),
            PathBuf::from("/srv/print-manager/branding").join(identifier)
        );
        for unsafe_value in [
            "../secret.png",
            "/etc/passwd",
            "branding/logo.png",
            "not-a-generated-id",
        ] {
            assert!(storage.managed_logo_path(unsafe_value).is_err());
        }
        assert_eq!(
            storage.logo_path_for_pdf("/legacy/shop-logo.png"),
            PathBuf::from("/legacy/shop-logo.png")
        );
    }

    #[tokio::test]
    async fn streams_hashes_limits_and_audits_artifacts() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let storage = Storage::new(directory.path().to_path_buf(), 4);
        storage
            .prepare()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mut staged = storage
            .begin_attachment()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        staged
            .write(b"test")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            staged.write(b"!").await,
            Err(StorageError::TooLarge { limit: 4 })
        ));
        let stored = storage
            .commit_attachment(staged)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(stored.byte_size, 4);
        assert_eq!(
            stored.sha256,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
        let report = storage
            .audit(&[])
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(report.orphaned, vec![stored.stored_name]);

        let interrupted = storage
            .begin_attachment()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let missing = AttachmentIntegrity {
            stored_name: "missing-generated-name".into(),
            byte_size: 10,
            sha256: "0".repeat(64),
        };
        let report = storage
            .audit(&[missing])
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(report.missing, vec!["missing-generated-name"]);
        assert_eq!(report.staging.len(), 1);
        storage.discard(interrupted).await;
    }

    #[tokio::test]
    async fn audit_reports_size_and_digest_corruption_without_mutating_files() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let storage = Storage::new(directory.path().to_path_buf(), 16);
        storage
            .prepare()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mut staged = storage
            .begin_attachment()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        staged
            .write(b"clean")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let stored = storage
            .commit_attachment(staged)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let metadata = AttachmentIntegrity {
            stored_name: stored.stored_name.clone(),
            byte_size: i64::try_from(stored.byte_size).unwrap_or(i64::MAX),
            sha256: stored.sha256,
        };
        fs::write(&stored.path, b"dirty")
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let report = storage
            .audit(std::slice::from_ref(&metadata))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(report.corrupt, vec![metadata.stored_name]);
        assert_eq!(fs::read(&stored.path).await.unwrap_or_default(), b"dirty");
    }
}
