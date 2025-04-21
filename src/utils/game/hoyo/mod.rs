use std::{fs, path::PathBuf};
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
}