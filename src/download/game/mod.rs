#[cfg(feature = "download")]
pub mod hoyo;
#[cfg(feature = "download")]
pub mod kuro;

pub struct Game;
pub trait Hoyo {
    fn download(urls: Vec<String>, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool;
    fn patch(url: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool;
    fn repair_game(res_list: String, game_path: String, is_fast: bool, progress: impl Fn(u64, u64) + Send + 'static) -> bool;
    fn repair_audio(res_list: String, locale: String, game_path: String, is_fast: bool, progress: impl Fn(u64, u64) + Send + 'static) -> bool;
}

#[allow(async_fn_in_trait)]
pub trait Kuro {
    async fn download<F>(manifest: String, chunk_base: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static;
    async fn patch<F>(manifest: String, version: String, chunk_base: String, game_path: String, preloaded: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static;
    async fn repair_game<F>(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static;
    async fn preload(manifest: String, version: String, chunk_base: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool;
}

#[allow(async_fn_in_trait)]
pub trait Sophon {
    async fn download<F>(manifest: String, chunk_base: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static;
    async fn patch(manifest: String, version: String, chunk_base: String, game_path: String, hpatchz_path: String, preloaded: bool, progress: impl Fn(u64, u64) + Send + 'static) -> bool;
    async fn repair_game<F>(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static;
    async fn preload(manifest: String, version: String, chunk_base: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool;

}