use std::fs;
use std::path::{Path, PathBuf};
use crate::download::game::Repairer;
use crate::utils::game::hoyo::{list_integrity_files, try_get_unused_files};

impl Repairer {
    pub fn repair_game(res_list: String, game_path: String, is_fast: bool) -> bool {
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

    pub fn repair_audio(res_list: String, locale: String, game_path: String, is_fast: bool) -> bool {
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

    pub fn remove_unused(game_path: String, used_files: Vec<String>, skip_list: Vec<String>) -> bool {
        let diff = try_get_unused_files(Path::new(game_path.as_str()).to_path_buf(), used_files.iter().map(|x| Path::new(x.as_str()).to_path_buf()).collect::<Vec<PathBuf>>(), skip_list.iter().map(|x| Path::new(x.as_str()).to_path_buf()).collect::<Vec<PathBuf>>());
        if diff.is_some() {
            let d = diff.unwrap();
            for f in d {
                if f.is_dir() {
                    fs::remove_dir_all(f).unwrap();
                } else {
                    fs::remove_file(f).unwrap();
                }
            }
            true
        } else {
            false
        }
    }
}
