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
    async fn download(url: String, game_path: String, use_patching_structure: bool, use_repair_structure: bool, progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool;
    async fn patch(url: String, game_path: String, progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool;
    async fn repair_game(res_list: String, game_path: String, is_fast: bool, progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool;
}

#[allow(async_fn_in_trait)]
pub trait Kuro {
    async fn download<F>(manifest: String, base_url: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn patch<F>(manifest: String, base_resources: String, base_zip: String, game_path: String, preloaded: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn repair_game<F>(manifest: String, base_url: String, game_path: String, is_fast: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn preload<F>(manifest: String, base_resources: String, base_zip: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
}

#[allow(async_fn_in_trait)]
pub trait Sophon {
    async fn download<F>(manifest: String, chunk_base: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn patch<F>(manifest: String, version: String, chunk_base: String, game_path: String, preloaded: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn repair_game<F>(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
    async fn preload<F>(manifest: String, version: String, chunk_base: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static;
}
