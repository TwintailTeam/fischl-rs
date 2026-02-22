use super::free_space;
use crate::utils::prettify_bytes;
use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{RANGE, USER_AGENT};
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::RetryTransientMiddleware;
use reqwest_retry::policies::ExponentialBackoff;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Duration;
use std::time::Instant;
use thiserror::Error;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// Thread-safe speed tracker using Exponential Moving Average for smooth readings.
/// This aggregates download speeds across multiple parallel downloads and provides
/// a smoothed speed value that doesn't fluctuate wildly.
pub struct SpeedTracker {
    total_bytes: AtomicU64,
    last_bytes: AtomicU64,
    last_update: StdMutex<Instant>,
    ema_speed: AtomicU64, // Stored as integer (bytes/s) for atomicity
}

impl SpeedTracker {
    pub fn new() -> Self {
        Self {
            total_bytes: AtomicU64::new(0),
            last_bytes: AtomicU64::new(0),
            last_update: StdMutex::new(Instant::now()),
            ema_speed: AtomicU64::new(0),
        }
    }

    /// Add bytes downloaded by a worker. Returns the current smoothed speed.
    pub fn add_bytes(&self, bytes: u64) -> u64 {
        self.total_bytes.fetch_add(bytes, AtomicOrdering::SeqCst);
        self.ema_speed.load(AtomicOrdering::SeqCst)
    }

    /// Update the EMA speed calculation. Call this periodically (e.g., every 200ms).
    /// Returns the smoothed speed in bytes/second.
    pub fn update(&self) -> u64 {
        const EMA_ALPHA: f64 = 0.25; // Lower alpha = heavier smoothing, less responsive to spikes
        const MIN_UPDATE_MS: u128 = 200; // More responsive speed display

        let now = Instant::now();
        let mut last_update = self.last_update.lock().unwrap();
        let elapsed = now.duration_since(*last_update);

        if elapsed.as_millis() < MIN_UPDATE_MS {
            return self.ema_speed.load(AtomicOrdering::SeqCst);
        }

        let current_bytes = self.total_bytes.load(AtomicOrdering::SeqCst);
        let last_bytes = self.last_bytes.swap(current_bytes, AtomicOrdering::SeqCst);
        let bytes_diff = current_bytes.saturating_sub(last_bytes);

        // Calculate instantaneous speed
        let elapsed_secs = elapsed.as_secs_f64();
        let instant_speed = if elapsed_secs > 0.0 {
            (bytes_diff as f64 / elapsed_secs) as u64
        } else {
            0
        };

        // Apply EMA smoothing
        let prev_ema = self.ema_speed.load(AtomicOrdering::SeqCst);
        let new_ema = if prev_ema == 0 {
            instant_speed
        } else {
            ((EMA_ALPHA * instant_speed as f64) + ((1.0 - EMA_ALPHA) * prev_ema as f64)) as u64
        };

        self.ema_speed.store(new_ema, AtomicOrdering::SeqCst);
        *last_update = now;

        new_ema
    }

    /// Get the current smoothed speed without updating.
    pub fn get_speed(&self) -> u64 {
        self.ema_speed.load(AtomicOrdering::SeqCst)
    }

    /// Get the total cumulative bytes tracked.
    pub fn get_total(&self) -> u64 {
        self.total_bytes.load(AtomicOrdering::SeqCst)
    }

    /// Reset the tracker for reuse.
    pub fn reset(&self) {
        self.total_bytes.store(0, AtomicOrdering::SeqCst);
        self.last_bytes.store(0, AtomicOrdering::SeqCst);
        self.ema_speed.store(0, AtomicOrdering::SeqCst);
        *self.last_update.lock().unwrap() = Instant::now();
    }
}

impl Default for SpeedTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct TokenBucketState {
    tokens: u64,
    last_refill: tokio::time::Instant,
}

/// Token bucket rate limiter for smooth bandwidth control.
/// Uses elapsed-based replenishment with 100ms ticks and 64KB minimum capacity,
#[derive(Debug)]
pub struct DownloadRateLimiter {
    limit_bps: AtomicU64,
    state: Mutex<TokenBucketState>,
}

impl DownloadRateLimiter {
    /// Tick interval: 100ms (10 times per second)
    /// Longer intervals reduce lock contention and produce smoother flow.
    const TICK_INTERVAL_SECS: f64 = 0.1;
    const MIN_CAPACITY: u64 = 64 * 1024; // 64 KiB minimum bucket size

    fn new() -> Self {
        Self { limit_bps: AtomicU64::new(0), state: Mutex::new(TokenBucketState { tokens: 0, last_refill: tokio::time::Instant::now() }) }
    }

    fn set_limit_bps(&self, limit_bps: u64) {
        self.limit_bps.store(limit_bps, AtomicOrdering::Relaxed);
    }

    async fn consume(&self, bytes: u64, cancel_token: Option<&AtomicBool>) -> bool {
        let limit_bps = self.limit_bps.load(AtomicOrdering::Relaxed);
        if limit_bps == 0 || bytes == 0 { return true; }

        let capacity = ((limit_bps as f64 * Self::TICK_INTERVAL_SECS) as u64).max(Self::MIN_CAPACITY);
        let rate = limit_bps as f64;
        let mut remaining = bytes;

        while remaining > 0 {
            if let Some(token) = cancel_token { if token.load(AtomicOrdering::Relaxed) { return false; } }

            let mut state = self.state.lock().await;
            let now = tokio::time::Instant::now();
            let elapsed = now.duration_since(state.last_refill).as_secs_f64();

            // Elapsed-based refill: add tokens proportional to time passed, cap at capacity
            let refilled = (state.tokens as f64 + elapsed * rate).min(capacity as f64) as u64;
            state.tokens = refilled;
            state.last_refill = now;

            if state.tokens == 0 {
                drop(state);
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }

            let allowed = state.tokens.min(remaining);
            state.tokens -= allowed;
            remaining -= allowed;

            if remaining > 0 {
                // Sleep proportional to missing tokens, clamped to 10ms-100ms
                let missing_secs = remaining as f64 / rate;
                let sleep_ms = (missing_secs * 1000.0).clamp(10.0, 100.0) as u64;
                drop(state);
                tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
            }
        }
        true
    }
}

static GLOBAL_DOWNLOAD_RATE_LIMITER: OnceLock<DownloadRateLimiter> = OnceLock::new();

fn global_download_rate_limiter() -> &'static DownloadRateLimiter {
    GLOBAL_DOWNLOAD_RATE_LIMITER.get_or_init(DownloadRateLimiter::new)
}

/// Global semaphore to limit concurrent active downloads.
/// Always enforced to prevent unbounded connections from saturating the network.
static GLOBAL_DOWNLOAD_SEMAPHORE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();

/// Default concurrent streams (no rate limit). High enough for throughput, bounded to prevent saturation.
/// Btw Xiao is the best :eeveecool:
const MAX_CONCURRENT_DOWNLOADS_UNLIMITED: usize = 24;
/// Concurrent streams when rate limiting is active. Lower count ensures rate limiter can control throughput.
const MAX_CONCURRENT_DOWNLOADS_LIMITED: usize = 12;

fn global_download_semaphore() -> &'static tokio::sync::Semaphore {
    GLOBAL_DOWNLOAD_SEMAPHORE.get_or_init(|| tokio::sync::Semaphore::new(MAX_CONCURRENT_DOWNLOADS_UNLIMITED))
}

/// Sets the global download speed limit in KB/s. Set to 0 for unlimited.
///
/// This limiter is shared across all concurrent downloads running inside this process.
pub fn set_global_download_speed_limit_kb(kb_per_sec: u64) {
    let bps = kb_per_sec.saturating_mul(1000);
    global_download_rate_limiter().set_limit_bps(bps);
}

pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024; // 64 KiB

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
    Reqwest(String),
    #[error("Connection error: {0}")]
    ConnectionError(String),
    #[error("Download cancelled")]
    Cancelled,
}

impl From<reqwest::Error> for DownloadingError {
    fn from(error: reqwest::Error) -> Self {
        DownloadingError::Reqwest(error.to_string())
    }
}

#[derive(Debug)]
pub struct AsyncDownloader {
    uri: String,
    length: Option<u64>,
    pub chunk_size: usize,
    pub continue_downloading: bool,
    pub check_free_space: bool,
    client: Arc<ClientWithMiddleware>,
    cancel_token: Option<Arc<AtomicBool>>,
}

impl AsyncDownloader {
    pub async fn setup_client(h1_only: bool) -> ClientWithMiddleware {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(30);
        let c = if h1_only { reqwest::Client::builder().pool_max_idle_per_host(2).http1_only().read_timeout(Duration::from_secs(60)).use_native_tls().build().unwrap() } else { reqwest::Client::builder().pool_max_idle_per_host(4).http2_adaptive_window(true).http2_keep_alive_interval(Duration::from_secs(30)).http2_keep_alive_timeout(Duration::from_secs(20)).read_timeout(Duration::from_secs(30)).use_native_tls().build().unwrap() };
        reqwest_middleware::ClientBuilder::new(c).with(RetryTransientMiddleware::new_with_policy(retry_policy)).build()
    }

    pub async fn new<T: AsRef<str>>(client: Arc<ClientWithMiddleware>, uri: T) -> Result<Self, DownloadingError> {
        let uri = uri.as_ref();
        // HEAD request to get content length - fail early if we can't reach the server
        let header = client.head(uri).header(USER_AGENT, "lib/fischl-rs").send().await.map_err(|e| DownloadingError::ConnectionError(e.to_string()))?;
        let length = header.headers().get("content-length").and_then(|len| len.to_str().ok()?.parse().ok());
        Ok(Self { uri: uri.to_owned(), length, chunk_size: DEFAULT_CHUNK_SIZE, continue_downloading: true, check_free_space: true, cancel_token: None, client })
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
    pub fn with_cancel_token(mut self, cancel_token: Option<Arc<AtomicBool>>) -> Self {
        self.cancel_token = cancel_token;
        self
    }
    pub fn length(&self) -> Option<u64> {
        self.length
    }

    pub async fn get_filename(&self) -> &str {
        let clean = self.uri.split('?').next().unwrap_or(&self.uri);
        if let Some(pos) = clean.replace('\\', "/").rfind(|c| c == '/') {
            if !clean[pos + 1..].is_empty() {
                return &clean[pos + 1..];
            }
        }
        "index.html"
    }

    pub async fn download(
        &mut self,
        path: impl Into<PathBuf>,
        mut progress: impl FnMut(u64, u64, u64, u64) + Send + Sync + 'static,
    ) -> Result<(), DownloadingError> {
        let path = path.into();
        let mut downloaded = 0;

        // Open or create output file
        let file = if path.exists() && self.continue_downloading {
            let mut file = tokio::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .await;

            // Continue downloading if the file exists and can be opened
            if let Ok(file) = &mut file {
                match file.metadata().await {
                    Ok(metadata) => {
                        // Stop the process if the file is already downloaded
                        if let Some(length) = self.length() {
                            match metadata.len().cmp(&length) {
                                std::cmp::Ordering::Less => (),
                                std::cmp::Ordering::Equal => return Ok(()),
                                // Trim downloaded file to prevent future issues (e.g. with extracting the archive)
                                std::cmp::Ordering::Greater => {
                                    if let Err(err) = file.set_len(length).await {
                                        return Err(DownloadingError::OutputFileError(
                                            path,
                                            err.to_string(),
                                        ));
                                    }
                                    return Ok(());
                                }
                            }
                        }

                        if let Err(err) =
                            file.seek(tokio::io::SeekFrom::Start(metadata.len())).await
                        {
                            return Err(DownloadingError::OutputFileError(path, err.to_string()));
                        }
                        downloaded = metadata.len() as usize;
                    }

                    Err(err) => {
                        return Err(DownloadingError::OutputFileMetadataError(
                            path,
                            err.to_string(),
                        ));
                    }
                }
            }

            file
        } else {
            let base_folder = path.parent().unwrap();
            if !base_folder.exists() {
                if let Err(err) = tokio::fs::create_dir_all(base_folder).await {
                    return Err(DownloadingError::OutputFileError(path, err.to_string()));
                }
            }
            tokio::fs::File::create(&path).await
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
                None => return Err(DownloadingError::PathNotMounted(path)),
            }
        }

        // Download data
        match file {
            Ok(mut file) => {
                // Check if download is already complete via HEAD request - if request fails, just proceed with download
                if let Ok(request) = self.client.head(&self.uri).header(RANGE, format!("bytes={downloaded}-")).header(USER_AGENT, "lib/fischl-rs").send().await {
                    // Request content range (downloaded + remained content size)
                    // If finished or overcame: bytes */10611646760
                    // If not finished: bytes 10611646759-10611646759/10611646760
                    // https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Range
                    if let Some(range) = request.headers().get("content-range") {
                        // Finish downloading if header says that we've already downloaded all the data
                        if range.to_str().unwrap_or("").contains("*/") {
                            progress(self.length.unwrap_or(downloaded as u64), self.length.unwrap_or(downloaded as u64), 0, 0);
                            return Ok(());
                        }
                    }
                }

                // Always acquire semaphore to bound concurrent connections.
                // When rate-limited, acquire extra permits to reduce concurrency further.
                let limit_active = global_download_rate_limiter().limit_bps.load(AtomicOrdering::Relaxed) > 0;
                let permits_needed: u32 = if limit_active { (MAX_CONCURRENT_DOWNLOADS_UNLIMITED / MAX_CONCURRENT_DOWNLOADS_LIMITED) as u32 } else { 1 };
                let semaphore = global_download_semaphore();
                let _permit = loop {
                    if let Some(token) = &self.cancel_token { if token.load(Ordering::Relaxed) { return Err(DownloadingError::Cancelled); } }
                    if let Ok(permit) = tokio::time::timeout(Duration::from_millis(100), semaphore.acquire_many(permits_needed)).await {
                        break permit.unwrap();
                    }
                };

                let request = match self.client.get(&self.uri).header(RANGE, format!("bytes={downloaded}-")).header(USER_AGENT, "lib/fischl-rs").send().await {
                    Ok(r) => r,
                    Err(e) => { return Err(DownloadingError::Reqwest(e.to_string())); }
                };

                // HTTP 416 = provided range is overcame actual content length (means file is downloaded)
                // I check this here because HEAD request can return 200 OK while GET - 416
                // https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/416
                if request.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                    progress(
                        self.length.unwrap_or(downloaded as u64),
                        self.length.unwrap_or(downloaded as u64),
                        0,
                        0,
                    );
                    return Ok(());
                }

                let mut stream = request.bytes_stream();
                let net_tracker = SpeedTracker::new();
                let disk_tracker = SpeedTracker::new();
                let mut last_update = Instant::now();
                let mut written_bytes = downloaded as u64;

                loop {
                    // Check cancel token before waiting for data
                    if let Some(token) = &self.cancel_token {
                        if token.load(Ordering::Relaxed) {
                            return Err(DownloadingError::Cancelled);
                        }
                    }

                    // Use timeout to avoid blocking indefinitely on slow connections
                    // This allows responsive pause even when network is stalled
                    let chunk_result =
                        tokio::time::timeout(Duration::from_millis(250), stream.next()).await;

                    let chunk = match chunk_result {
                        Ok(Some(data)) => data,
                        Ok(None) => break, // Stream ended normally
                        Err(_) => continue, // Timeout - loop back to check cancel token
                    };

                    let data = chunk?;

                    // Rate-limit the entire network chunk at once (no 8KB slicing)
                    if !global_download_rate_limiter().consume(data.len() as u64, self.cancel_token.as_deref()).await {
                        return Err(DownloadingError::Cancelled);
                    }

                    // Track and write the full chunk
                    net_tracker.add_bytes(data.len() as u64);
                    if let Err(e) = file.write_all(&data).await { return Err(DownloadingError::OutputFileError(path, e.to_string())); }
                    written_bytes += data.len() as u64;
                    disk_tracker.add_bytes(data.len() as u64);

                    let now = Instant::now();
                    if now.duration_since(last_update).as_millis() >= 500 {
                        let net_speed = net_tracker.update();
                        let disk_speed = disk_tracker.update();
                        progress(written_bytes, self.length.unwrap_or(written_bytes), net_speed, disk_speed);
                        last_update = now;
                    }
                }

                // Always emit a final progress callback to ensure all bytes are reported
                // This is critical for small/fast downloads where the 500ms callback may not fire
                let net_speed = net_tracker.update();
                let disk_speed = disk_tracker.update();
                progress(
                    written_bytes,
                    self.length.unwrap_or(written_bytes),
                    net_speed,
                    disk_speed,
                );

                if let Err(err) = file.flush().await {
                    return Err(DownloadingError::OutputFileError(path, err.to_string()));
                }
                drop(file);
                Ok(())
            }
            Err(err) => Err(DownloadingError::OutputFileError(path, err.to_string())),
        }
    }
}
