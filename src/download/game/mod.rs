#[cfg(feature = "download")]
pub mod hoyo;
#[cfg(feature = "download")]
pub mod kuro;
#[cfg(feature = "download")]
pub mod zipped;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub struct Game;
#[allow(async_fn_in_trait)]
pub trait Zipped {
    async fn download(urls: Vec<String>, game_path: String, progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool;
    async fn patch(url: String, game_path: String, progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool;
    async fn repair_game(res_list: String, game_path: String, is_fast: bool, progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool;
}

/// Progress callback parameters for Kuro (same as Sophon for consistency):
/// (download_current, download_total, install_current, install_total, net_speed, disk_speed, phase)
/// - download_current/total: network bytes downloaded
/// - install_current/total: bytes moved/installed to final location
/// - phase: 0=idle, 1=verifying, 2=downloading, 3=installing, 4=validating, 5=moving
#[allow(async_fn_in_trait)]
pub trait Kuro {
    async fn download<F>(manifest: String, base_url: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn patch<F>(manifest: String, base_resources: String, base_zip: String, game_path: String, preloaded: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn repair_game<F>(manifest: String, base_url: String, game_path: String, is_fast: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn preload<F>(manifest: String, base_resources: String, base_zip: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
}

/// Progress callback parameters for Sophon:
/// (download_current, download_total, install_current, install_total, net_speed, disk_speed, phase)
/// - download_current/total: network bytes downloaded
/// - install_current/total: bytes written to disk and validated
/// - phase: 0=idle, 1=verifying, 2=downloading, 3=installing, 4=validating, 5=moving
#[allow(async_fn_in_trait)]
pub trait Sophon {
    async fn download<F>(manifest: String, chunk_base: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn patch<F>(manifest: String, version: String, chunk_base: String, game_path: String, hpatchz_path: String, preloaded: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn repair_game<F>(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn preload<F>(manifest: String, version: String, chunk_base: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
}
