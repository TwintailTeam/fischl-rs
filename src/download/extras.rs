use std::fs;
use std::path::Path;
use std::sync::Arc;
use crate::download::Extras;
use crate::utils::{get_codeberg_release, get_github_release};
use crate::utils::downloader::{AsyncDownloader};
use crate::utils::github_structs::Asset;

#[cfg(feature = "download")]
impl Extras {
    pub async fn download_fps_unlock(repository: String, dest: String, progress: impl Fn(u64, u64) + Send + Sync + 'static) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let rel = tokio::task::spawn_blocking(move || { get_github_release(repository.clone()) }).await.unwrap();
            if rel.is_some() {
                let r = rel.unwrap();
                let u = r.assets.get(0).unwrap().browser_download_url.clone();
                let c = AsyncDownloader::setup_client().await;
                let da = AsyncDownloader::new(Arc::new(c), u).await;
                if da.is_ok() {
                    let mut du = da.unwrap();
                    let dl = du.download(d.join("fpsunlock.exe").as_path(), progress).await;
                    if dl.is_ok() { true } else { false }
                } else { false }
            } else { false }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }

    pub async fn download_jadeite(repository: String, dest: String, progress: impl Fn(u64, u64) + Send + Sync + 'static) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let rel = tokio::task::spawn_blocking(move || { get_codeberg_release(repository.clone()) }).await.unwrap();
            if rel.is_some() {
                let r = rel.unwrap();
                let u = r.get(0).unwrap().assets.get(0).unwrap().browser_download_url.clone();
                let c = AsyncDownloader::setup_client().await;
                let da = AsyncDownloader::new(Arc::new(c), u).await;
                if da.is_ok() {
                    let mut du = da.unwrap();
                    let dl = du.download(d.join("jadeite.zip").as_path(), progress).await;
                    if dl.is_ok() { true } else { false }
                } else { false }
            } else { false }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }

    pub async fn download_xxmi(repository: String, dest: String, with_loader: bool, progress: impl Fn(u64, u64) + Send + Sync + 'static) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let rel = tokio::task::spawn_blocking(move || { get_github_release(repository.clone()) }).await.unwrap();
            if rel.is_some() {
                let r = rel.unwrap();
                let filtered = r.assets.into_iter().filter(|a| a.name.clone().unwrap().to_ascii_lowercase().contains("xxmi")).collect::<Vec<Asset>>();
                let u = filtered.get(0).unwrap().clone().browser_download_url.clone();
                let c = Arc::new(AsyncDownloader::setup_client().await);
                let mut da = AsyncDownloader::new(c.clone(), u).await;
                if da.is_ok() {
                    let mut du = da.unwrap();
                    let dl = du.download(d.join("xxmi.zip").as_path(), progress).await;
                    if dl.is_ok() {
                        if with_loader {
                            let r1 = tokio::task::spawn_blocking(move || { get_github_release("TwintailTeam/3dmloader-Package".to_string()) }).await.unwrap();
                            if r1.is_some() {
                                let r = r1.unwrap();
                                let u = r.assets.get(0).unwrap().clone().browser_download_url.clone();
                                da = AsyncDownloader::new(c, u).await;
                                if da.is_ok() {
                                    let mut du = da.unwrap();
                                    let dl = du.download(d.join("3dmloader.exe").as_path(), |_c, _t| {}).await;
                                    if dl.is_ok() { true } else { false }
                                } else { false }
                            } else { false }
                        } else { true }
                    } else { false }
                } else { false }
            } else { false }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }

    pub async fn download_xxmi_packages(gimi_repo: String, srmi_repo: String, zzmi_repo: String, wwmi_repo: String, himi_repo: String, dest: String) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
                let gimi = tokio::task::spawn_blocking(move || { get_github_release(gimi_repo.clone()) }).await.unwrap();
                let srmi = tokio::task::spawn_blocking(move || { get_github_release(srmi_repo.clone()) }).await.unwrap();
                let zzmi = tokio::task::spawn_blocking(move || { get_github_release(zzmi_repo.clone()) }).await.unwrap();
                let wwmi = tokio::task::spawn_blocking(move || { get_github_release(wwmi_repo.clone()) }).await.unwrap();
                let himi = tokio::task::spawn_blocking(move || { get_github_release(himi_repo.clone()) }).await.unwrap();

                if gimi.is_some() && srmi.is_some() && zzmi.is_some() && wwmi.is_some() && himi.is_some() {
                    let gi = gimi.unwrap();
                    let sr = srmi.unwrap();
                    let zz = zzmi.unwrap();
                    let ww = wwmi.unwrap();
                    let hi = himi.unwrap();

                    let dlg = gi.assets.get(0).unwrap().clone().browser_download_url;
                    let dlsr = sr.assets.get(0).unwrap().clone().browser_download_url;
                    let dlzz = zz.assets.get(0).unwrap().clone().browser_download_url;
                    let dlww = ww.assets.get(0).unwrap().clone().browser_download_url;
                    let dlhi = hi.assets.get(0).unwrap().clone().browser_download_url;

                    let mut status: Vec<bool> = vec![];
                    let c = Arc::new(AsyncDownloader::setup_client().await);
                    let mut dg = AsyncDownloader::new(c.clone(), dlg).await;
                    if dg.is_ok() {
                        let mut du = dg.unwrap();
                        let dl = du.download(d.join("gimi.zip").as_path(), |_c, _t| {}).await;
                        status.push(dl.is_ok());
                    }

                    dg = AsyncDownloader::new(c.clone(), dlsr).await;
                    if dg.is_ok() {
                        let mut du = dg.unwrap();
                        let dl = du.download(d.join("srmi.zip").as_path(), |_c, _t| {}).await;
                        status.push(dl.is_ok());
                    }

                    dg = AsyncDownloader::new(c.clone(), dlzz).await;
                    if dg.is_ok() {
                        let mut du = dg.unwrap();
                        let dl = du.download(d.join("zzmi.zip").as_path(), |_c, _t| {}).await;
                        status.push(dl.is_ok());
                    }

                    dg = AsyncDownloader::new(c.clone(), dlww).await;
                    if dg.is_ok() {
                        let mut du = dg.unwrap();
                        let dl = du.download(d.join("wwmi.zip").as_path(), |_c, _t| {}).await;
                        status.push(dl.is_ok());
                    }

                    dg = AsyncDownloader::new(c.clone(), dlhi).await;
                    if dg.is_ok() {
                        let mut du = dg.unwrap();
                        let dl = du.download(d.join("himi.zip").as_path(), |_c, _t| {}).await;
                        status.push(dl.is_ok());
                    }
                    status.iter().all(|&b| b)
                } else {
                    false
                }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }
}