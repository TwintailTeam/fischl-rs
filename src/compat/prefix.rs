use std::fs;
use std::ops::Add;
use std::path::Path;
use wincompatlib::prelude::{WineBootExt, WineWithExt};
use wincompatlib::wine::{Wine, WineArch};
use crate::compat::Compat;
use crate::utils::downloader::Downloader;
use crate::utils::{extract_archive, get_full_extension};

#[cfg(feature = "compat")]
impl Compat {
    pub fn download_runner(url: String, dest: String, extract: bool) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let mut downloader = Downloader::new(url).unwrap();
            let fin = downloader.get_filename();
            let ext = get_full_extension(fin).unwrap();
            let name = String::from("runner.").add(ext);
            let dp = d.to_path_buf().join(name.as_str());
            let dl = downloader.download(dp.clone(), |_, _| {});
            if dl.is_ok() {
                if extract {
                    let r = extract_archive(dp.to_str().unwrap().to_string(), d.to_str().unwrap().to_string(), true);
                    r
                } else {
                    dl.is_ok()
                }
            } else { false }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }

    pub fn setup_prefix(wine: String, prefix: String) -> Result<Self, String> {
        let wine = Wine::from_binary(wine).with_prefix(prefix).with_arch(WineArch::Win64);
        let wp = wine.init_prefix(None::<&str>);
        if wp.is_ok() {
            Ok(Compat { wine })
        } else {
            Err("Failed to create wine prefix!".into())
        }
    }

    pub fn update_prefix(wine: String, prefix: String) -> Result<bool, String> {
        let upd = Wine::from_binary(wine).with_arch(WineArch::Win64).update_prefix(Some(prefix));

        if upd.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn end_session(wine: String, prefix: String) -> Result<bool, String> {
        let session = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).end_session();
        if session.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn shutdown(wine: String, prefix: String) -> Result<bool, String> {
        let session = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).shutdown();
        if session.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn reboot(wine: String, prefix: String) -> Result<bool, String> {
        let session = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).restart();
        if session.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn stop_processes(wine: String, prefix: String, force: bool) -> Result<bool, String> {
        let session = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).stop_processes(force);
        if session.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
