use std::fs;
use std::path::Path;
use std::sync::Arc;
use crate::download::Extras;
use crate::utils::{TTLManifest,downloader::AsyncDownloader};

#[cfg(feature = "download")]
impl Extras {
    pub async fn download_extra_package(package_id: String, package_type: String, extract_mode: bool, move_subdirs: bool, append_package_to_dest: bool, dest: String, progress: impl Fn(u64, u64) + Send + Sync + 'static) -> bool {
        let d = Path::new(&dest);
        if d.exists() {
            let pkg_id = package_id.clone();
            let manifest = tokio::task::spawn_blocking(move || { Self::fetch_ttl_manifest(pkg_id.clone()) }).await.unwrap();
            if let Some(m) = manifest {
                if m.retcode != 0 { return false; }
                // Data is an optional object response does not contain data if its invalid
                if let Some(mf) = m.data {
                    if let Some(pkg) = mf.packages.iter().find(|e| e.package_name.to_ascii_lowercase().contains(package_type.as_str())) {
                        match pkg.default_download_mode.as_str() {
                            "DOWNLOAD_MODE_FILE" => {
                                let c = AsyncDownloader::setup_client().await;
                                let da = AsyncDownloader::new(Arc::new(c), pkg.git_url.clone()).await;
                                if da.is_ok() {
                                    let mut du = da.unwrap();
                                    let dl = du.download(d.join(&pkg.package_name).as_path(), progress).await;
                                    if dl.is_ok() {
                                        let ver_file = d.join("VERSION.txt");
                                        if let Err(_) = fs::write(ver_file, format!("{}={}", package_id.clone().to_ascii_uppercase(), &pkg.version).as_bytes()) { return false; }
                                        if extract_mode {
                                            let ext = crate::utils::extract_archive(d.join(&pkg.package_name).to_str().unwrap().to_string(), dest.clone(), move_subdirs);
                                            if ext { true } else { false }
                                        } else { true }
                                    } else { false }
                                } else { false }
                            },
                            "DOWNLOAD_MODE_RAW" => {
                                if pkg.file_list.is_empty() { return false; }
                                let progress = Arc::new(progress);
                                let c = Arc::new(AsyncDownloader::setup_client().await);
                                for f in pkg.file_list.clone() {
                                    let progress_cb = progress.clone();
                                    if f.ends_with("/") {
                                        let dir_path = if append_package_to_dest { d.join(&package_type).join(f.trim_end_matches('/')) } else { d.join(f.trim_end_matches('/')) };
                                        if let Err(_) = fs::create_dir_all(&dir_path) { return false; }
                                    } else {
                                        let dest_path = if append_package_to_dest { d.join(&package_type).join(&f) } else { d.join(&f) };
                                        if let Some(parent) = dest_path.parent() { if let Err(_) = fs::create_dir_all(parent) { return false; } }

                                        let url = format!("{base}/{f}", base = pkg.raw_url.trim_end_matches("/"));
                                        let da = AsyncDownloader::new(c.clone(), url).await;
                                        if da.is_ok() {
                                            let mut du = da.unwrap();
                                            let dl = du.download(dest_path.as_path(), move |cur, total| { progress_cb(cur, total) }).await;
                                            if dl.is_err() { return false; }
                                        } else { return false; }
                                    }
                                }
                                true
                            },
                            _ => { false }
                        }
                    } else { false }
                } else { false }
            } else { false }
        } else { false }
    }

    pub fn fetch_ttl_manifest(package: String) -> Option<TTLManifest> {
        if package.is_empty() { None } else {
            let url = format!("https://dl-public.twintaillauncher.app/launcher_app/manifests/{package}.json");
            let req = reqwest::blocking::get(url);
            match req {
                Ok(response) => {
                    match response.json::<TTLManifest>() {
                        Ok(version_text) => { Some(version_text) },
                        Err(_) => { None },
                    }
                }
                Err(_) => { None },
            }
        }
    }
}