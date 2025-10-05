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
pub fn download_steamrt(path: PathBuf, dest: PathBuf, edition: String, branch: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
    if !path.exists() || edition.is_empty() || branch.is_empty() { false } else {
        let url = format!("https://repo.steampowered.com/steamrt-images-{edition}/snapshots/{branch}/SteamLinuxRuntime_{edition}.tar.xz");
        let mut dl = Downloader::new(url).unwrap();
        let p = path.join("steamrt.tar.xz");
        let dld = dl.download(p.as_path(), progress);
        if dld.is_ok() { extract_archive("".to_string(), p.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string(), true); }
        true
    }
}

#[cfg(feature = "compat")]
pub fn check_steamrt_update(edition: String, branch: String) -> Option<String> {
    if edition.is_empty() || branch.is_empty() { None } else {
        let url = format!("https://repo.steampowered.com/steamrt-images-{edition}/snapshots/{branch}/VERSION.txt");
        let req = reqwest::blocking::get(url);
        match req {
            Ok(response) => {
                match response.text() {
                    Ok(version_text) => { Some(version_text) },
                    Err(_) => { None },
                }
            }
            Err(_) => { None },
        }
    }
}