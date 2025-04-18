use std::fs;
use std::path::Path;
use reqwest::header::USER_AGENT;
use crate::download::Downloader;
use crate::utils::{get_github_release, get_tukanrepo_release};


impl Downloader {
    #[cfg(feature = "download")]
    pub fn download_xxmi(repository: String, dest: &Path) -> Option<bool> {
        if dest.exists() {
            let rel = get_github_release(repository.clone());
            if rel.is_some() {
                let client = reqwest::blocking::Client::new();

                let r = rel.unwrap();
                let dl = r.assets.get(0).unwrap().clone().browser_download_url;
                let response = client.get(dl).header(USER_AGENT, "lib/fischl-rs").send().unwrap();

                if response.status().is_success() {
                    let filename = dest.join("xxmi.zip");
                    fs::write(&filename, response.bytes().unwrap()).unwrap();
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

    #[cfg(feature = "download")]
    pub fn download_xxmi_loader(repository: String, dest: &Path, delete_dll: bool) -> Option<bool> {
        if dest.exists() {
            let rel = get_tukanrepo_release(repository.clone());
            if rel.is_some() {
                let client = reqwest::blocking::Client::new();

                let r = rel.unwrap();
                let dl = r.get(0).unwrap().assets.get(0).unwrap().browser_download_url.clone();
                let response = client.get(dl).header(USER_AGENT, "lib/fischl-rs").send().unwrap();

                if response.status().is_success() {
                    let filename = dest.join("3dmloader.exe");
                    fs::write(&filename, response.bytes().unwrap()).unwrap();

                    if delete_dll {
                        fs::remove_file(&filename.to_str().unwrap().replace(".exe", ".dll")).unwrap();
                    }
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

    #[cfg(feature = "download")]
    pub fn download_xxmi_packages(gimi_repo: String, srmi_repo: String, zzmi_repo: String, wwmi_repo: String, dest: &Path, use_fork: bool) -> Option<bool> {
        if dest.exists() {
            if use_fork {
                let gimi = get_tukanrepo_release(gimi_repo.clone());
                let srmi = get_tukanrepo_release(srmi_repo.clone());
                let zzmi = get_tukanrepo_release(zzmi_repo.clone());
                let wwmi = get_tukanrepo_release(wwmi_repo.clone());

                if gimi.is_some() && srmi.is_some() && zzmi.is_some() && wwmi.is_some() {
                    let client = reqwest::blocking::Client::new();

                    let gi = gimi.unwrap();
                    let sr = srmi.unwrap();
                    let zz = zzmi.unwrap();
                    let ww = wwmi.unwrap();

                    let dlg = gi.get(0).unwrap().assets.get(0).unwrap().clone().browser_download_url;
                    let dlsr = sr.get(0).unwrap().assets.get(0).unwrap().clone().browser_download_url;
                    let dlzz = zz.get(0).unwrap().assets.get(0).unwrap().clone().browser_download_url;
                    let dlww = ww.get(0).unwrap().assets.get(0).unwrap().clone().browser_download_url;

                    let response0 = client.get(dlg).header(USER_AGENT, "lib/fischl-rs").send().unwrap();
                    let response1 = client.get(dlsr).header(USER_AGENT, "lib/fischl-rs").send().unwrap();
                    let response2 = client.get(dlzz).header(USER_AGENT, "lib/fischl-rs").send().unwrap();
                    let response3 = client.get(dlww).header(USER_AGENT, "lib/fischl-rs").send().unwrap();

                    if response0.status().is_success() && response1.status().is_success() && response2.status().is_success() && response3.status().is_success() {
                        fs::write(&dest.join("gimi.zip"), response0.bytes().unwrap()).unwrap();
                        fs::write(&dest.join("srmi.zip"), response1.bytes().unwrap()).unwrap();
                        fs::write(&dest.join("zzmi.zip"), response2.bytes().unwrap()).unwrap();
                        fs::write(&dest.join("wwmi.zip"), response3.bytes().unwrap()).unwrap();
                        Some(true)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                let gimi = get_github_release(gimi_repo.clone());
                let srmi = get_github_release(srmi_repo.clone());
                let zzmi = get_github_release(zzmi_repo.clone());
                let wwmi = get_github_release(wwmi_repo.clone());

                if gimi.is_some() && srmi.is_some() && zzmi.is_some() && wwmi.is_some() {
                    let client = reqwest::blocking::Client::new();

                    let gi = gimi.unwrap();
                    let sr = srmi.unwrap();
                    let zz = zzmi.unwrap();
                    let ww = wwmi.unwrap();

                    let dlg = gi.assets.get(0).unwrap().clone().browser_download_url;
                    let dlsr = sr.assets.get(0).unwrap().clone().browser_download_url;
                    let dlzz = zz.assets.get(0).unwrap().clone().browser_download_url;
                    let dlww = ww.assets.get(0).unwrap().clone().browser_download_url;

                    let response0 = client.get(dlg).header(USER_AGENT, "lib/fischl-rs").send().unwrap();
                    let response1 = client.get(dlsr).header(USER_AGENT, "lib/fischl-rs").send().unwrap();
                    let response2 = client.get(dlzz).header(USER_AGENT, "lib/fischl-rs").send().unwrap();
                    let response3 = client.get(dlww).header(USER_AGENT, "lib/fischl-rs").send().unwrap();

                    if response0.status().is_success() && response1.status().is_success() && response2.status().is_success() && response3.status().is_success() {
                        fs::write(&dest.join("gimi.zip"), response0.bytes().unwrap()).unwrap();
                        fs::write(&dest.join("srmi.zip"), response1.bytes().unwrap()).unwrap();
                        fs::write(&dest.join("zzmi.zip"), response2.bytes().unwrap()).unwrap();
                        fs::write(&dest.join("wwmi.zip"), response3.bytes().unwrap()).unwrap();
                        Some(true)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        } else {
            fs::create_dir_all(&dest).unwrap();
            None
        }
    }
}