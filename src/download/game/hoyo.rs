use std::path::{Path};
use crate::download::game::{Game, Hoyo};
use crate::utils::downloader::Downloader;
use crate::utils::game::hoyo::{list_integrity_files};

impl Hoyo for Game {
    fn download(urls: Vec<String>, game_path: String) -> bool {
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

    fn patch(url: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
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

    fn repair_game(res_list: String, game_path: String, is_fast: bool) -> bool {
        let files = list_integrity_files(res_list, "pkg_version".parse().unwrap());

        if files.is_some() {
            let f = files.unwrap();
            f.iter().for_each(|file| {
                let path = Path::new(game_path.as_str());
                if is_fast {
                    let rslt= file.fast_verify(path.to_path_buf().clone());
                    if !rslt {
                        file.repair(path.to_path_buf());
                    }
                } else {
                    let rslt = file.verify(path.to_path_buf().clone());
                    if !rslt {
                        file.repair(path.to_path_buf());
                    }
                }
            });
            true
        } else {
            false
        }
    }

    fn repair_audio(res_list: String, locale: String, game_path: String, is_fast: bool) -> bool {
        let files = list_integrity_files(res_list, format!("Audio_{}_pkg_version", locale));

        if files.is_some() {
            let f = files.unwrap();
            f.iter().for_each(|file| {
                let path = Path::new(game_path.as_str());
                if is_fast {
                    let rslt = file.fast_verify(path.to_path_buf().clone());
                    if !rslt {
                        file.repair(path.to_path_buf());
                    }
                } else {
                    let rslt = file.verify(path.to_path_buf().clone());
                    if !rslt {
                        file.repair(path.to_path_buf());
                    }
                }
            });
            true
        } else {
            false
        }
    }
}