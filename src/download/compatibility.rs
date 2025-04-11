use std::fs;
use std::path::Path;
use reqwest::header::USER_AGENT;

#[cfg(feature = "download")]
pub fn download_runner(url: String, dest: &Path) -> Option<bool> {
    if dest.exists() {
        let client = reqwest::blocking::Client::new();
        let response = client.get(url).header(USER_AGENT, "KeqingLauncher/tauri-app").send();
        if response.is_ok() {
            let r = response.unwrap();
            fs::write(&dest.join("runner.zip"), r.bytes().unwrap()).unwrap();
            Some(true)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(feature = "download")]
pub fn download_dxvk(url: String, dest: &Path) -> Option<bool> {
    if dest.exists() {
        let client = reqwest::blocking::Client::new();
        let response = client.get(url).header(USER_AGENT, "KeqingLauncher/tauri-app").send();
        if response.is_ok() {
            let r = response.unwrap();
            fs::write(&dest.join("dxvk.zip"), r.bytes().unwrap()).unwrap();
            Some(true)
        } else {
            None
        }
    } else {
        None
    }
}