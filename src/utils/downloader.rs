use crate::utils::prettify_bytes;
use std::io::{Write, Seek};
use std::path::PathBuf;
use std::fs::File;
use std::sync::Arc;
use reqwest::header::RANGE;
use reqwest::StatusCode;
use serde::{Serialize, Deserialize};
use thiserror::Error;

use super::free_space;

pub const DEFAULT_CHUNK_SIZE: usize = 128 * 1024; // 128 KB

#[derive(Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadingError {
    #[error("Path is not mounted: {0:?}")]
    PathNotMounted(PathBuf),
    #[error("No free space available for specified path: {0:?} (requires {}, available {})", prettify_bytes(*.1), prettify_bytes(*.2))]
    NoSpaceAvailable(PathBuf, u64, u64),
    #[error("Failed to create output file {0:?}: {1}")]
    OutputFileError(PathBuf, String),
    #[error("Failed to read metadata of the output file {0:?}: {1}")]
    OutputFileMetadataError(PathBuf, String),
    #[error("Request error: {0}")]
    Reqwest(String)
}

impl From<reqwest::Error> for DownloadingError {
    fn from(error: reqwest::Error) -> Self {
        DownloadingError::Reqwest(error.to_string())
    }
}

#[derive(Debug)]
pub struct Downloader {
    uri: String,
    length: Option<u64>,
    pub chunk_size: usize,
    pub continue_downloading: bool,
    pub check_free_space: bool
}

impl Downloader {
    pub fn new<T: AsRef<str>>(uri: T) -> Result<Self, reqwest::Error> {
        let uri = uri.as_ref();

        let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(60)).build()?;
        let header = client.head(uri).send()?;
        let length = header.headers().get("content-length").map(|len| len.to_str().unwrap().parse().expect("Requested site's content-length is not a number"));

        Ok(Self {
            uri: uri.to_owned(),
            length,
            chunk_size: DEFAULT_CHUNK_SIZE,
            continue_downloading: true,
            check_free_space: true
        })
    }

    #[inline]
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    #[inline]
    pub fn with_continue_downloading(mut self, continue_downloading: bool) -> Self {
        self.continue_downloading = continue_downloading;
        self
    }

    #[inline]
    pub fn with_free_space_check(mut self, check_free_space: bool) -> Self {
        self.check_free_space = check_free_space;
        self
    }

    #[inline]
    pub fn length(&self) -> Option<u64> {
        self.length
    }

    /// Get name of downloading file from uri
    /// - `https://example.com/example.zip` -> `example.zip`
    /// - `https://example.com` -> `index.html`
    pub fn get_filename(&self) -> &str {
        if let Some(pos) = self.uri.replace('\\', "/").rfind(|c| c == '/') {
            if !self.uri[pos + 1..].is_empty() {
                return &self.uri[pos + 1..];
            }
        }
        "index.html"
    }

    pub fn download(&mut self, path: impl Into<PathBuf>, progress: impl Fn(u64, u64) + Send + 'static) -> Result<(), DownloadingError> {
        let path = path.into();
        let mut downloaded = 0;

        // Open or create output file
        let file = if path.exists() && self.continue_downloading {
            let mut file = std::fs::OpenOptions::new().read(true).write(true).open(&path);

            // Continue downloading if the file exists and can be opened
            if let Ok(file) = &mut file {
                match file.metadata() {
                    Ok(metadata) => {
                        // Stop the process if the file is already downloaded
                        if let Some(length) = self.length() {
                            match metadata.len().cmp(&length) {
                                std::cmp::Ordering::Less => (),
                                std::cmp::Ordering::Equal => return Ok(()),
                                // Trim downloaded file to prevent future issues (e.g. with extracting the archive)
                                std::cmp::Ordering::Greater => {
                                    if let Err(err) = file.set_len(length) {
                                        return Err(DownloadingError::OutputFileError(path, err.to_string()));
                                    }
                                    return Ok(());
                                }
                            }
                        }

                        if let Err(err) = file.seek(std::io::SeekFrom::Start(metadata.len())) {
                            return Err(DownloadingError::OutputFileError(path, err.to_string()));
                        }

                        downloaded = metadata.len() as usize;
                    }

                    Err(err) => return Err(DownloadingError::OutputFileMetadataError(path, err.to_string()))
                }
            }

            file
        } else {
            let base_folder = path.parent().unwrap();
            if !base_folder.exists() {
                if let Err(err) = std::fs::create_dir_all(base_folder) {
                    return Err(DownloadingError::OutputFileError(path, err.to_string()));
                }
            }

            File::create(&path)
        };

        // Check available free space
        if self.check_free_space {
            match free_space::available(&path) {
                Some(space) => {
                    if let Some(required) = self.length() {
                        let required = required.checked_sub(downloaded as u64).unwrap_or_default();
                        if space < required {
                            return Err(DownloadingError::NoSpaceAvailable(path, required, space));
                        }
                    }
                }

                None => return Err(DownloadingError::PathNotMounted(path))
            }
        }

        // Download data
        match file {
            Ok(mut file) => {
                let mut chunk = Vec::with_capacity(self.chunk_size);
                let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(60)).build()?;
                let request = client.head(&self.uri).header(RANGE, format!("bytes={downloaded}-")).send()?;

                // Request content range (downloaded + remained content size)
                // If finished or overcame: bytes */10611646760
                // If not finished: bytes 10611646759-10611646759/10611646760
                // https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Range
                if let Some(range) = request.headers().get("content-range") {
                    // Finish downloading if header says that we've already downloaded all the data
                    if range.to_str().unwrap().contains("*/") {
                        (progress)(self.length.unwrap_or(downloaded as u64), self.length.unwrap_or(downloaded as u64));
                        return Ok(());
                    }
                }

                let request = client.get(&self.uri).header(RANGE, format!("bytes={downloaded}-")).send()?;

                // HTTP 416 = provided range is overcame actual content length (means file is downloaded)
                // I check this here because HEAD request can return 200 OK while GET - 416
                // https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/416
                if request.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                    (progress)(self.length.unwrap_or(downloaded as u64), self.length.unwrap_or(downloaded as u64));
                    return Ok(());
                }

                let bytes = request.bytes()?;
                let mut stream = bytes.iter();
                let p = Arc::new(progress);

                while let Some(byte) = stream.next() {
                    chunk.push(*byte);

                    if chunk.len() == self.chunk_size {
                        if let Err(err) = file.write_all(&chunk) {
                            return Err(DownloadingError::OutputFileError(path, err.to_string()));
                        }

                        chunk.clear();
                        downloaded += self.chunk_size;
                        (p)(downloaded as u64, self.length.unwrap_or(downloaded as u64));
                    }
                }

                if !chunk.is_empty() {
                    if let Err(err) = file.write_all(&chunk) {
                        return Err(DownloadingError::OutputFileError(path, err.to_string()));
                    }

                    downloaded += chunk.len();
                    (p)(downloaded as u64, downloaded as u64);
                }
                Ok(())
            }

            Err(err) => Err(DownloadingError::OutputFileError(path, err.to_string()))
        }
    }
}
