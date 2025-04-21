use crate::utils::KuroFile;

#[cfg(feature = "download")]
pub mod hoyo;
#[cfg(feature = "download")]
pub mod kuro;

pub struct Game;
pub trait Hoyo {
    fn download(urls: Vec<String>, game_path: String) -> bool;
    fn patch(url: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool;
    fn repair_game(res_list: String, game_path: String, is_fast: bool) -> bool;
    fn repair_audio(res_list: String, locale: String, game_path: String, is_fast: bool) -> bool;
}

pub trait Kuro {
    fn download(urls: Vec<KuroFile>, game_path: String) -> bool;
    fn patch(url: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool;
    fn repair_game(index_url:String, res_list: String, game_path: String, is_fast: bool) -> bool;
}