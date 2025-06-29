use std::fs;
use std::ops::Add;
use std::path::Path;
use wincompatlib::dxvk::{InstallParams};
use wincompatlib::prelude::{WineWithExt};
use wincompatlib::wine::{Wine, WineArch};
use crate::compat::Compat;
use crate::utils::downloader::Downloader;
use crate::utils::{extract_archive, get_full_extension};

#[cfg(feature = "compat")]
impl Compat {
    pub fn download_dxvk(url: String, dest: String, extract: bool) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let mut downloader = Downloader::new(url).unwrap();
            let fin = downloader.get_filename();
            let ext = get_full_extension(fin).unwrap();
            let name = String::from("dxvk.").add(ext);
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

    pub fn add_dxvk(wine: String, prefix: String, dxvk: String, repair_dlls: bool) -> Result<bool, String> {
        let wine = Wine::from_binary(wine).with_prefix(prefix).with_arch(WineArch::Win64);
        let ip = InstallParams {
            dxgi: true,
            d3d9: true,
            d3d10core: true,
            d3d11: true,
            repair_dlls,
            arch: Default::default(),
        };

        let wp = wine.install_dxvk(dxvk, ip);
        if wp.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn remove_dxvk(wine: String, prefix: String) -> Result<bool, String> {
        let upd = Wine::from_binary(wine).with_arch(WineArch::Win64).with_prefix(prefix).uninstall_dxvk(InstallParams::default());
        if upd.is_ok() {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}