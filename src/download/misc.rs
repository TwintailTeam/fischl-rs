use std::fs;
use std::path::Path;
use reqwest::header::USER_AGENT;
use crate::download::Downloader;
use crate::utils::{get_codeberg_release};

#[cfg(feature = "download")]
impl Downloader {
    pub fn download_fps_unlock(repository: String, dest: &Path) -> Option<bool> {
        if dest.exists() {
            let rel = get_codeberg_release(repository.clone());
            if rel.is_some() {
                let client = reqwest::blocking::Client::new();

                let r = rel.unwrap();
                let dl = r.get(0).unwrap().assets.get(0).unwrap().browser_download_url.clone();
                let response = client.get(dl).header(USER_AGENT, "lib/fischl-rs").send().unwrap();

                if response.status().is_success() {
                    let filename = dest.join("fpsunlock.exe");
                    fs::write(&filename, response.bytes().unwrap()).unwrap();
                    Some(true)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            fs::create_dir_all(&dest).unwrap();
            None
        }
    }

    pub fn download_jadeite(repository: String, dest: &Path) -> Option<bool> {
        if dest.exists() {
            let rel = get_codeberg_release(repository.clone());
            if rel.is_some() {
                let client = reqwest::blocking::Client::new();

                let r = rel.unwrap();
                let dl = r.get(0).unwrap().assets.get(0).unwrap().browser_download_url.clone();
                let response = client.get(dl).header(USER_AGENT, "lib/fischl-rs").send().unwrap();

                if response.status().is_success() {
                    let filename = dest.join("jadeite.zip");
                    fs::write(&filename, response.bytes().unwrap()).unwrap();
                    Some(true)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            fs::create_dir_all(&dest).unwrap();
            None
        }
    }
}