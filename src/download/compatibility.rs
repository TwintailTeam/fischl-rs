use std::fs;
use std::path::Path;
use crate::download::{Compatibility};
use crate::utils::downloader::Downloader;

#[cfg(feature = "download")]
impl Compatibility {
    pub fn download_runner(url: String, dest: String) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let mut downloader = Downloader::new(url).unwrap();
            let dl = downloader.download(d.to_path_buf().join("runner.zip"), |_, _| {});

            if dl.is_ok() {
                true
            } else {
                false
            }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }

    pub fn download_dxvk(url: String, dest: String) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let mut downloader = Downloader::new(url).unwrap();
            let dl = downloader.download(d.to_path_buf().join("dxvk.zip"), |_, _| {});

            if dl.is_ok() {
                true
            } else {
                false
            }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }
}