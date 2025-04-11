use std::fs;
use std::path::Path;
use reqwest::header::USER_AGENT;
use crate::utils::get_github_release;

#[cfg(feature = "download")]
pub fn download_xxmi(dest: &Path) -> Option<bool> {
    if dest.exists() {
        let rel = get_github_release("SpectrumQT".parse().unwrap(), "XXMI-Libs-Package".parse().unwrap());
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
pub fn download_xxmi_packages(dest: &Path) -> Option<bool> {
    if dest.exists() {
        let gimi = get_github_release("SilentNightSound".parse().unwrap(), "GIMI-Package".parse().unwrap());
        let srmi = get_github_release("SpectrumQT".parse().unwrap(), "SRMI-Package".parse().unwrap());
        let zzmi = get_github_release("leotorrez".parse().unwrap(), "ZZMI-Package".parse().unwrap());
        let wwmi = get_github_release("SpectrumQT".parse().unwrap(), "WWMI-Package".parse().unwrap());

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
    } else {
        fs::create_dir_all(&dest).unwrap();
        None
    }
}