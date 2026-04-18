#[cfg(feature = "compat")]
use std::ops::Add;
#[cfg(feature = "compat")]
use std::path::PathBuf;
#[cfg(feature = "compat")]
use crate::utils::downloader::AsyncDownloader;
#[cfg(feature = "compat")]
use crate::utils::extract_archive_with_progress;

#[cfg(feature = "compat")]
pub async fn download_steamrt(path: PathBuf, dest: PathBuf, edition: String, branch: String, progress: impl FnMut(u64, u64, u64, u64) + Send + Sync + 'static, extract_progress: impl Fn(u64, u64) + Send + 'static) -> bool {
    if !path.exists() || edition.is_empty() || branch.is_empty() { return false; }
    if crate::utils::steamrt_up_to_date(dest.as_path(), edition.clone(), branch.clone()) == Some(true) { return true; }

    let code = match edition.as_str() { "steamrt3" => "sniper", "steamrt4" => "4", _ => return false };
    let archive = if cfg!(target_arch = "aarch64") { format!("SteamLinuxRuntime_{code}-arm64.tar.xz") } else if cfg!(target_arch = "x86_64") { format!("SteamLinuxRuntime_{code}.tar.xz") } else { return false; };
    let p = path.join(&archive);
    let expected_hash = match tokio::task::spawn_blocking({ let edition = edition.clone(); let branch = branch.clone(); move || get_steamrt_checksums(edition, branch) }).await { Ok(Some(h)) => h, _ => return false };

    const MAX_ATTEMPTS: u8 = 3;
    let mut downloaded = false;
    let progress = std::sync::Arc::new(std::sync::Mutex::new(progress));
    for _attempt in 1..=MAX_ATTEMPTS {
        let url = format!("https://repo.steampowered.com/{edition}/images/{branch}/{archive}");
        let cl = AsyncDownloader::setup_client(true).await;
        let dl = AsyncDownloader::new(std::sync::Arc::new(cl), url).await;
        let Ok(mut d) = dl else { continue; };
        let prc = progress.clone();

        match d.download(p.as_path(), move |first, second, third, fourth| {
            let mut pl = prc.lock().unwrap();
            pl(first, second, third, fourth);
        }).await {
            Ok(_) => {
                match tokio::fs::read(&p).await {
                    Ok(bytes) => {
                        use sha2::{Digest, Sha256};
                        let actual_hash = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();
                        if actual_hash == expected_hash { downloaded = true; break; } else { let _ = tokio::fs::remove_file(&p).await; }
                    }
                    Err(_e) => { let _ = tokio::fs::remove_file(&p).await; }
                }
            }
            Err(_e) => { let _ = tokio::fs::remove_file(&p).await; }
        }
    }
    if !downloaded { return false; }
    extract_archive_with_progress(p.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string(), true, extract_progress)
}

#[cfg(feature = "compat")]
pub(crate) fn get_steamrt_version(edition: String, branch: String) -> Option<String> {
    if edition.is_empty() || branch.is_empty() { return None; }
    let url = format!("https://repo.steampowered.com/{edition}/images/{branch}/VERSION.txt");
    reqwest::blocking::get(url).ok()?.text().ok()
}

#[cfg(feature = "compat")]
fn get_steamrt_checksums(edition: String, branch: String) -> Option<String> {
    let text = reqwest::blocking::Client::new().get(format!("https://repo.steampowered.com/{edition}/images/{branch}/SHA256SUMS")).header(reqwest::header::ACCEPT, "text/plain").send().ok()?.text().ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(pos) = line.find('*') {
            let hash = &line[..pos].trim();
            let file = &line[pos + 1..].trim();
            if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                #[cfg(target_arch = "aarch64")]
                if file.ends_with("-arm64.tar.xz") { return Some(hash.to_string()); }
                #[cfg(target_arch = "x86_64")]
                if file.ends_with(".tar.xz") && !file.ends_with("-arm64.tar.xz") { return Some(hash.to_string()); }
            }
        }
    }
    None
}

#[cfg(feature = "compat")]
pub async fn download_runner(url: String, dest: String, extract: bool, progress: impl FnMut(u64, u64, u64, u64) + Send + Sync + 'static, extract_progress: impl Fn(u64, u64) + Send + 'static) -> bool {
    let d = std::path::Path::new(&dest);
    if d.exists() {
        let c = AsyncDownloader::setup_client(false).await;
        let dl = AsyncDownloader::new(std::sync::Arc::new(c), url).await;
        if dl.is_ok() {
            let mut dll = dl.unwrap();
            let fin = dll.get_filename().await;
            let ext = crate::utils::get_full_extension(fin).unwrap();
            let name = String::from("runner.").add(ext);
            let dp = d.to_path_buf().join(name.as_str());
            let dla = dll.download(dp.clone(), progress).await;
            if dla.is_ok() {
                if extract { extract_archive_with_progress(dp.to_str().unwrap().to_string(), d.to_str().unwrap().to_string(), true, extract_progress) } else { true }
            } else { false }
        } else { false }
    } else {
        let r = std::fs::create_dir_all(d);
        match r {
            Ok(_) => { false }
            Err(_) => { false }
        }
    }
}