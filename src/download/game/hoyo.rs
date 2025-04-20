use std::path::{Path};
use crate::download::game::{Game};
use crate::utils::downloader::Downloader;

impl Game {
    pub fn download(urls: Vec<String>, game_path: String) -> bool {
        if urls.is_empty() || game_path.is_empty() {
            return false;
        }

        for url in urls {
            let mut downloader = Downloader::new(url).unwrap();
            let file = downloader.get_filename().to_string();
            downloader.download(Path::new(game_path.as_str()).to_path_buf().join(&file), |_, _| {}).unwrap();

        }
        true
    }

    pub fn patch(url: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if url.is_empty() || game_path.is_empty() {
            return false;
        }

        let mut downloader = Downloader::new(url).unwrap();
        let file = downloader.get_filename().to_string();
        let dl = downloader.download(Path::new(game_path.as_str()).to_path_buf().join(&file), progress);

        if dl.is_ok() {
            true
        } else {
            false
        }
    }
}