use std::path::Path;
use std::sync::{Arc, Mutex};
use crate::download::game::{Game, Zipped};
use crate::utils::downloader::{AsyncDownloader};

impl Zipped for Game {
    async fn download(urls: Vec<String>, game_path: String, progress: impl Fn(u64, u64) + Send + Sync + 'static) -> bool {
        if urls.is_empty() || game_path.is_empty() { return false; }

        let mut ret = true;
        let progress = Arc::new(Mutex::new(progress));
        for url in urls {
            let p = progress.clone();
            let c = AsyncDownloader::setup_client().await;
            let dla = AsyncDownloader::new(Arc::new(c), url).await;
            if dla.is_ok() {
                let mut dlu = dla.unwrap();
                let file = dlu.get_filename().await.to_string();
                let dl = dlu.download(Path::new(game_path.as_str()).to_path_buf().join(&file), move |current, total| {
                    let pl = p.lock().unwrap();
                    pl(current, total);
                }).await;
                if dl.is_ok() { ret = true; } else { ret = false; }
            } else { ret = false; }
        }
        ret
    }

    async fn patch(_url: String, _game_path: String, _progress: impl Fn(u64, u64) + Send + Sync + 'static) -> bool {
        true
    }

    async fn repair_game(_res_list: String, _game_path: String, _is_fast: bool, _progress: impl Fn(u64, u64) + Send + Sync + 'static) -> bool {
        true
    }
}