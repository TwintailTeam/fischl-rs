use std::fs;
use std::path::Path;
use crate::download::Extras;
use crate::utils::{get_codeberg_release, get_github_release};
use crate::utils::downloader::Downloader;
use crate::utils::github_structs::Asset;

#[cfg(feature = "download")]
impl Extras {
    pub fn download_fps_unlock(repository: String, dest: String) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let rel = get_github_release(repository.clone());
            if rel.is_some() {
                let r = rel.unwrap();
                let u = r.assets.get(0).unwrap().browser_download_url.clone();
                let mut downloader = Downloader::new(u).unwrap();
                let dl = downloader.download(d.join("fpsunlock.exe").to_path_buf(), |_, _| {});
                dl.is_ok()
            } else {
                false
            }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }

    pub fn download_jadeite(repository: String, dest: String) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let rel = get_codeberg_release(repository.clone());
            if rel.is_some() {
                let r = rel.unwrap();
                let u = r.get(0).unwrap().assets.get(0).unwrap().browser_download_url.clone();
                let mut downloader = Downloader::new(u).unwrap();
                let dl = downloader.download(d.join("jadeite.zip").to_path_buf(), |_, _| {});
                dl.is_ok()
            } else {
                false
            }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }

    pub fn download_xxmi(repository: String, dest: String, with_loader: bool) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let rel = get_github_release(repository.clone());
            if rel.is_some() {
                let r = rel.unwrap();
                let filtered = r.assets.into_iter().filter(|a| a.name.to_ascii_lowercase().contains("xxmi")).collect::<Vec<Asset>>();
                let u = filtered.get(0).unwrap().clone().browser_download_url.clone();
                let mut downloader = Downloader::new(u).unwrap();
                let dl = downloader.download(d.join("xxmi.zip").to_path_buf(), |_, _| {});

                if dl.is_ok() {
                    if with_loader {
                        let r1 = get_github_release("TwintailTeam/3dmloader-Package".to_string());
                        if r1.is_some() {
                            let r = r1.unwrap();
                            let u = r.assets.get(0).unwrap().clone().browser_download_url.clone();
                            downloader = Downloader::new(u).unwrap();
                            downloader.download(d.join("3dmloader.exe").to_path_buf(), |_, _| {}).unwrap();
                        }
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }

    pub fn download_xxmi_packages(gimi_repo: String, srmi_repo: String, zzmi_repo: String, wwmi_repo: String, himi_repo: String, dest: String) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
                let gimi = get_github_release(gimi_repo.clone());
                let srmi = get_github_release(srmi_repo.clone());
                let zzmi = get_github_release(zzmi_repo.clone());
                let wwmi = get_github_release(wwmi_repo.clone());
                let himi = get_github_release(himi_repo.clone());

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

                    let mut downloader = Downloader::new(dlg).unwrap();
                    let dl = downloader.download(d.join("gimi.zip").to_path_buf(), move |_, _| {});

                    let mut downloader1 = Downloader::new(dlsr).unwrap();
                    let dl1 = downloader1.download(d.join("srmi.zip").to_path_buf(), move |_, _| {});

                    let mut downloader2 = Downloader::new(dlzz).unwrap();
                    let dl2 = downloader2.download(d.join("zzmi.zip").to_path_buf(), move |_, _| {});

                    let mut downloader3 = Downloader::new(dlww).unwrap();
                    let dl3 = downloader3.download(d.join("wwmi.zip").to_path_buf(), move |_, _| {});

                    let mut downloader4 = Downloader::new(dlhi).unwrap();
                    let dl4 = downloader4.download(d.join("himi.zip").to_path_buf(), move |_, _| {});
                    dl.is_ok() && dl1.is_ok() && dl2.is_ok() && dl3.is_ok() && dl4.is_ok()
                } else {
                    false
                }
        } else {
            fs::create_dir_all(d).unwrap();
            false
        }
    }
}