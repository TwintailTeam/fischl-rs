#[cfg(feature = "compat")]
use std::path::PathBuf;
#[cfg(feature = "compat")]
use crate::utils::downloader::AsyncDownloader;
#[cfg(feature = "compat")]
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
pub async fn download_steamrt(path: PathBuf, dest: PathBuf, edition: String, branch: String, progress: impl Fn(u64, u64) + Send + Sync + 'static) -> bool {
    if !path.exists() || edition.is_empty() || branch.is_empty() { false } else {
        let code = if edition.as_str() == "steamrt3" { "sniper" } else { "4" };
        let url = format!("https://repo.steampowered.com/{edition}/images/{branch}/SteamLinuxRuntime_{code}.tar.xz");
        let cl = AsyncDownloader::setup_client().await;
        let dl = AsyncDownloader::new(std::sync::Arc::new(cl), url).await;
        if dl.is_ok() {
            let mut d = dl.unwrap();
            let p = path.join("steamrt.tar.xz");
            let dld = d.download(p.as_path(), progress).await;
            if dld.is_ok() {
                extract_archive("".to_string(), p.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string(), true);
                true
            } else { false }
        } else { false }
    }
}

#[cfg(feature = "compat")]
pub fn check_steamrt_update(edition: String, branch: String) -> Option<String> {
    if edition.is_empty() || branch.is_empty() { None } else {
        let url = format!("https://repo.steampowered.com/{edition}/images/{branch}/VERSION.txt");
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