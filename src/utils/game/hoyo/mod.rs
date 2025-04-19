use std::{fs, path::PathBuf};
use std::collections::HashSet;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use crate::utils::downloader::Downloader;

pub mod voice_locale;

// {"remoteName": "UnityPlayer.dll", "md5": "8c8c3d845b957e4cb84c662bed44d072", "fileSize": 33466104}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct IntegrityFile {
    pub path: PathBuf,
    pub md5: String,
    pub size: u64,
    pub base_url: String
}

pub fn list_integrity_files(res_list_url: String, file: String) -> Option<Vec<IntegrityFile>> {
    let client = reqwest::blocking::Client::new();
    let response = client.get(format!("{}/{}", res_list_url, file)).send();

    let mut files = Vec::new();

    if response.is_ok() {
        let r = response.unwrap();

        for line in String::from_utf8_lossy(r.bytes().unwrap().iter().as_ref()).lines() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                files.push(IntegrityFile {
                    path: PathBuf::from(value["remoteName"].as_str().unwrap()),
                    md5: value["md5"].as_str().unwrap().to_string(),
                    size: value["fileSize"].as_u64().unwrap(),
                    base_url: res_list_url.clone()
                });
            }
        }

        Some(files)
    } else {
        None
    }
}

impl IntegrityFile {
    pub(crate) fn verify<T: Into<PathBuf> + std::fmt::Debug>(&self, game_path: T) -> bool {
        let file_path: PathBuf = game_path.into().join(&self.path);
        let Ok(metadata) = file_path.metadata() else {
            return false;
        };

        if metadata.len() != self.size {
            false
        } else {
            match fs::read(&file_path) {
                Ok(hash) => format!("{:x}", Md5::digest(hash)).to_ascii_lowercase() == self.md5.to_ascii_lowercase(),
                Err(_) => false
            }
        }
    }

    pub(crate) fn fast_verify<T: Into<PathBuf> + std::fmt::Debug>(&self, game_path: T) -> bool {
        fs::metadata(game_path.into().join(&self.path)).and_then(|metadata| Ok(metadata.len() == self.size)).unwrap_or(false)
    }

    pub(crate) fn repair<T: Into<PathBuf> + std::fmt::Debug>(&self, game_path: T) -> Option<bool> {
        let mut downloader = Downloader::new(format!("{}/{}", self.base_url, self.path.to_string_lossy())).unwrap();
        downloader.continue_downloading = false;

        let dl = downloader.download(game_path.into().join(&self.path), |_, _| {});

        if dl.is_ok() {
            Some(true)
        } else {
            None
        }
    }

    /// Calculate difference between actual files stored in `game_dir`, and files listed in `used_files`
    /// Returned difference will contain files that are not used by the game and should (or just can) be deleted
    /// `used_files` can be both absolute and relative to `game_dir`
    pub(crate) fn try_get_unused_files<T, F, U>(game_dir: T, used_files: F, skip_names: U) -> Option<Vec<PathBuf>> where T: Into<PathBuf>, F: IntoIterator<Item = PathBuf>, U: IntoIterator<Item = String> {
        fn list_files(path: PathBuf, skip_names: &[String]) -> std::io::Result<Vec<PathBuf>> {
            let mut files = Vec::new();

            for entry in fs::read_dir(&path)? {
                let entry = entry?;
                let entry_path = path.join(entry.file_name());

                let mut should_skip = false;

                for skip in skip_names {
                    if entry.file_name().to_string_lossy().contains(skip) {
                        should_skip = true;
                        break;
                    }
                }

                if !should_skip {
                    if entry.file_type()?.is_dir() {
                        files.append(&mut list_files(entry_path, skip_names)?);
                    } else {
                        files.push(entry_path);
                    }
                }
            }

            Ok(files)
        }

        let used_files = used_files.into_iter().map(|path| path.into()).collect::<HashSet<PathBuf>>();
        let skip_names = skip_names.into_iter().collect::<Vec<String>>();
        let game_dir = game_dir.into();

        Some(list_files(game_dir.clone(), skip_names.as_slice()).unwrap()
            .into_iter()
            .filter(move |path| {
                // File persist in used_files => unused
                if used_files.contains(path) {
                    return false;
                }

                // File persist in used_files => unused
                if let Ok(path) = path.strip_prefix(&game_dir) {
                    if used_files.contains(path) {
                        return false;
                    }
                }

                // File not persist in used_files => not unused
                return true;
            }).collect())
    }
}