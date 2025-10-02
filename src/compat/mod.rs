use std::path::PathBuf;
use crate::utils::downloader::Downloader;
use crate::utils::extract_archive;
#[cfg(feature = "compat")]
use wincompatlib::wine::Wine;

#[cfg(feature = "compat")]
pub mod prefix;
#[cfg(feature = "compat")]
pub mod dxvk;

#[cfg(feature = "compat")]
pub struct Compat {
    pub wine: Wine,
}

#[cfg(feature = "compat")]
pub fn download_steamrt(path: PathBuf, dest: PathBuf, edition: String) -> bool {
    if !path.exists() || edition.is_empty() { false } else {
        let url = format!("https://repo.steampowered.com/steamrt-images-{edition}/snapshots/latest-container-runtime-public-beta/SteamLinuxRuntime_{edition}.tar.xz");
        let mut dl = Downloader::new(url).unwrap();
        let p = path.join("steamrt.tar.xz");
        let dld = dl.download(p.as_path(), |_, _| {});
        if dld.is_ok() { extract_archive("".to_string(), p.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string(), true); }
        true
    }
}