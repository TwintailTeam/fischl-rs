use std::{fs, path::PathBuf};
use serde::{Deserialize, Serialize};
use crate::utils::downloader::Downloader;
use crate::utils::KuroIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoiceLocale {
    English,
    Japanese,
    Korean,
    Chinese
}

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

pub fn list_kuro_integrity_files(index_url: String, res_list_url: String) -> Option<Vec<IntegrityFile>> {
    let client = reqwest::blocking::Client::new();
    let response = client.get(index_url.clone()).send();

    if response.is_ok() {
        let r = response.unwrap();
        let bv = r.bytes().unwrap().to_vec();
        let sd: KuroIndex = serde_json::from_slice(&bv).unwrap();

        let rsp: Vec<IntegrityFile> = sd.resource.into_iter().map(|resource| IntegrityFile {
            path: PathBuf::from(resource.dest.strip_prefix('/').unwrap_or(resource.dest.as_str())),
            md5: resource.md5,
            size: resource.size,
            base_url: res_list_url.clone()
        }).collect();
        Some(rsp)
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
                Ok(hash) => chksum_md5::chksum(hash).unwrap().to_hex_lowercase() == self.md5.to_ascii_lowercase(),
                Err(_) => false
            }
        }
    }

    pub(crate) fn fast_verify<T: Into<PathBuf> + std::fmt::Debug>(&self, game_path: T) -> bool {
        fs::metadata(game_path.into().join(&self.path)).and_then(|metadata| Ok(metadata.len() == self.size)).unwrap_or(false)
    }

    pub(crate) fn repair<T: Into<PathBuf> + std::fmt::Debug>(&self, game_path: T, progress: impl Fn(u64, u64) + Send + 'static) -> Option<bool> {
        let mut downloader = Downloader::new(format!("{}/{}", self.base_url, self.path.to_string_lossy())).unwrap();
        downloader.continue_downloading = false;
        let dl = downloader.download(game_path.into().join(&self.path), progress);

        if dl.is_ok() {
            Some(true)
        } else {
            None
        }
    }
}

impl VoiceLocale {
    #[inline]
    pub fn list() -> &'static [VoiceLocale] {
        &[Self::English, Self::Japanese, Self::Korean, Self::Chinese]
    }

    /// Convert enum value to its name
    ///
    /// `VoiceLocale::English` -> `English`
    #[inline]
    pub fn to_name(&self) -> &str {
        match self {
            Self::English  => "English",
            Self::Japanese => "Japanese",
            Self::Korean   => "Korean",
            Self::Chinese  => "Chinese"
        }
    }

    /// Convert enum value to its code
    ///
    /// `VoiceLocale::English` -> `en-us`
    #[inline]
    pub fn to_code(&self) -> &str {
        match self {
            Self::English  => "en-us",
            Self::Japanese => "ja-jp",
            Self::Korean   => "ko-kr",
            Self::Chinese  => "zh-cn"
        }
    }

    /// Convert enum value to its folder name
    ///
    /// `VoiceLocale::English` -> `English(US)`
    #[inline]
    pub fn to_folder(&self) -> &str {
        match self {
            Self::English  => "English(US)",
            Self::Japanese => "Japanese",
            Self::Korean   => "Korean",
            Self::Chinese  => "Chinese"
        }
    }

    /// Try to convert string to enum
    ///
    /// - `English` -> `VoiceLocale::English`
    /// - `English(US)` -> `VoiceLocale::English`
    /// - `en-us` -> `VoiceLocale::English`
    #[inline]
    pub fn from_str<T: AsRef<str>>(str: T) -> Option<Self> {
        match str.as_ref() {
            // Locales names
            "English"  => Some(Self::English),
            "Japanese" => Some(Self::Japanese),
            "Korean"   => Some(Self::Korean),
            "Chinese"  => Some(Self::Chinese),

            // Lowercased variants
            "english"  => Some(Self::English),
            "japanese" => Some(Self::Japanese),
            "korean"   => Some(Self::Korean),
            "chinese"  => Some(Self::Chinese),

            // Folders
            "English(US)" => Some(Self::English),
            "Chinese(PRC)" => Some(Self::Chinese),

            // Codes
            "en-us" => Some(Self::English),
            "ja-jp" => Some(Self::Japanese),
            "ko-kr" => Some(Self::Korean),
            "zh-cn" => Some(Self::Chinese),

            _ => None
        }
    }
}