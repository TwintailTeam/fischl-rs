use std::path::{Path};
use crate::download::game::Repairer;
use crate::utils::game::hoyo::list_integrity_files;

impl Repairer {
    pub fn repair_game(res_list: String, game_path: String, is_fast: bool) -> bool {
        let files = list_integrity_files(res_list, "pkg_version".parse().unwrap());

        if files.is_some() {
            let f = files.unwrap();
            f.iter().for_each(|file| {
                let path = Path::new(game_path.as_str());
                if is_fast {
                    file.fast_verify(path.to_path_buf().clone());
                } else {
                    file.verify(path.to_path_buf().clone());
                }
                file.repair(path.to_path_buf());
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
                    file.fast_verify(path.to_path_buf().clone());
                } else {
                    file.verify(path.to_path_buf().clone());
                }
                file.repair(path.to_path_buf());
            });
            true
        } else {
            false
        }
    }

    pub fn verify_game(res_list: String, game_path: String, is_fast: bool) -> bool {
        let files = list_integrity_files(res_list, "pkg_version".parse().unwrap());

        if files.is_some() {
            let f = files.unwrap();
            f.iter().for_each(|file| {
                let path = Path::new(game_path.as_str());
                if is_fast {
                    file.fast_verify(path.to_path_buf().clone());
                } else {
                    file.verify(path.to_path_buf().clone());
                }
            });
            true
        } else {
            false
        }
    }

    pub fn verify_audio(res_list: String, locale: String, game_path: String, is_fast: bool) -> bool {
        let files = list_integrity_files(res_list, format!("Audio_{}_pkg_version", locale));

        if files.is_some() {
            let f = files.unwrap();
            f.iter().for_each(|file| {
                let path = Path::new(game_path.as_str());
                if is_fast {
                    file.fast_verify(path.to_path_buf().clone());
                } else {
                    file.verify(path.to_path_buf().clone());
                }
            });
            true
        } else {
            false
        }
    }
}
