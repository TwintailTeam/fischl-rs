use std::fs;
use std::path::Path;
use reqwest::header::USER_AGENT;
use crate::download::Downloader;

#[cfg(feature = "download")]
impl Downloader {
    pub fn download_runner(url: String, dest: &Path) -> Option<bool> {
        if dest.exists() {
            let client = reqwest::blocking::Client::new();
            let response = client.get(url).header(USER_AGENT, "lib/fischl-rs").send();
            if response.is_ok() {
                let r = response.unwrap();
                let filename = dest.join("runner.zip");

                if r.status().is_success() {
                    fs::write(&filename, r.bytes().unwrap()).unwrap();
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

    pub fn download_dxvk(url: String, dest: &Path) -> Option<bool> {
        if dest.exists() {
            let client = reqwest::blocking::Client::new();
            let response = client.get(url).header(USER_AGENT, "lib/fischl-rs").send();
            if response.is_ok() {
                let r = response.unwrap();
                let filename = dest.join("dxvk.zip");

                if r.status().is_success() {
                    fs::write(&filename, r.bytes().unwrap()).unwrap();
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