use std::path::Path;
use std::sync::{Arc, Mutex};
use crate::download::game::{Game, Zipped};
use crate::utils::downloader::Downloader;

impl Zipped for Game {
    fn download(urls: Vec<String>, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if urls.is_empty() || game_path.is_empty() { return false; }

        let progress = Arc::new(Mutex::new(progress));
        for url in urls {
            let p = progress.clone();
            let mut downloader = Downloader::new(url).unwrap();
            let file = downloader.get_filename().to_string();
            downloader.download(Path::new(game_path.as_str()).to_path_buf().join(&file), move |current, total| {
                let pl = p.lock().unwrap();
                pl(current, total);
            }).unwrap();
        }
        true
    }

    fn patch(url: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if url.is_empty() || game_path.is_empty() { return false; }

        let mut downloader = Downloader::new(url).unwrap();
        let file = downloader.get_filename().to_string();
        let dl = downloader.download(Path::new(game_path.as_str()).to_path_buf().join(&file), progress);
        if dl.is_ok() { true } else { false }
    }

    fn repair_game(_res_list: String, _game_path: String, _is_fast: bool, _progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        true
    }
}