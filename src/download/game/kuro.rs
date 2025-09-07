use std::path::Path;
use std::sync::{Arc};
use std::sync::atomic::{AtomicU64, Ordering};
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt};
use crate::download::game::{Game, Kuro};
use crate::utils::downloader::{AsyncDownloader};
use crate::utils::{krpatchz, move_all, validate_checksum, KuroIndex};

impl Kuro for Game {
    async fn download<F>(manifest: String, chunk_base: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let p = Path::new(game_path.as_str()).to_path_buf();
        let dlp = p.join("downloading");
        let dlr = p.join("repairing");
        let dlptch = dlp.join("patching");

        if dlr.exists() { tokio::fs::remove_dir_all(&dlr).await.unwrap(); }
        if dlptch.exists() { tokio::fs::remove_dir_all(&dlptch).await.unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client().await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
        let dll = dl.download(dlp.clone().join("manifest.json"), |_, _| {}).await;

        if dll.is_ok() {
            let mut f = tokio::fs::File::open(dlp.join("manifest.json").as_path()).await.unwrap();
            let mut reader = String::new();
            f.read_to_string(&mut reader).await.unwrap();
            let files: KuroIndex = serde_json::from_str(&reader).unwrap();

            let staging = dlp.join("staging");
            if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }

            let total_bytes: u64 = files.resource.iter().map(|f| f.size).sum();
            let progress_counter = Arc::new(AtomicU64::new(0));
            let progress = Arc::new(progress);

            // Start of download code
            let file_tasks = futures::stream::iter(files.resource.into_iter().map(|ff| {
                let chunk_base = chunk_base.clone();
                let staging = staging.clone();
                let client = client.clone();
                let progress = progress.clone();
                let progress_counter = progress_counter.clone();
                async move {
                    let progress_counter = progress_counter.clone();
                    let progress = progress.clone();
                    let client = client.clone();

                    let spc = staging.clone();
                    let cb = chunk_base.clone();

                    let filen = ff.dest.strip_prefix("/").unwrap_or(&ff.dest);
                    let output_path = spc.join(filen);
                    let valid = validate_checksum(output_path.as_path(), ff.clone().md5.to_ascii_lowercase()).await;

                    // File exists in "staging" directory and checksum is valid skip it
                    // NOTE: This in theory will never be needed, but it is implemented to prevent redownload of already valid files as a form of "catching up"
                    if output_path.exists() && valid {
                        progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                        let processed = progress_counter.load(Ordering::SeqCst);
                        progress(processed, total_bytes);
                        return;
                    } else {
                        if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                    }

                    let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                    let dlf = dl.download(output_path.clone(), |_, _| {}).await;

                    if dlf.is_ok() && output_path.exists() {
                        let r1 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                        if r1 {
                            progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                            let processed = progress_counter.load(Ordering::SeqCst);
                            progress(processed, total_bytes);
                        } else {
                            // Retry to download if we fail
                            let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                            let dlf1 = dl.download(output_path.clone(), |_, _| {}).await;
                            if dlf1.is_ok() {
                                let r2 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                                if r2 {
                                    progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress(processed, total_bytes);
                                }
                            }
                        }
                    }
                }
            })).buffer_unordered(8).collect::<Vec<()>>();
            file_tasks.await;
            // All files are complete make sure we report done just in case
            progress(total_bytes, total_bytes);
            // Move from "staging" to "game_path" and delete "downloading" directory
            let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
            if moved.is_ok() { tokio::fs::remove_dir_all(dlp.as_path()).await.unwrap(); }
            true
        } else {
            false
        }
    }

    async fn patch<F>(manifest: String, version: String, chunk_base: String, game_path: String, krpatchz_path: String, preloaded: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() || version.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str());
        let p = mainp.join("patching");
        let dlp = mainp.join("downloading");
        let dlr = mainp.join("repairing");

        if dlr.exists() { tokio::fs::remove_dir_all(&dlr).await.unwrap(); }
        if dlp.exists() { tokio::fs::remove_dir_all(&dlp).await.unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client().await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
        let dll = dl.download(p.clone().join("manifest.json"), |_, _| {}).await;

        if dll.is_ok() {
            let mut f = tokio::fs::File::open(p.join("manifest.json").as_path()).await.unwrap();
            let mut reader = String::new();
            f.read_to_string(&mut reader).await.unwrap();
            let files: KuroIndex = serde_json::from_str(&reader).unwrap();

            let staging = p.join("staging");
            if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }

            let total_bytes: u64 = files.resource.iter().map(|f| f.size).sum();
            let progress_counter = Arc::new(AtomicU64::new(0));
            let progress = Arc::new(progress);

            if files.patch_infos.is_some() {
                // has krdiff
                if preloaded { true } else {
                    let fpi = files.patch_infos.unwrap();
                    let patch_lock = Arc::new(tokio::sync::Mutex::new(()));
                    // No preload
                    let file_tasks = futures::stream::iter(files.resource.into_iter().map(|ff| {
                        let chunk_base = chunk_base.clone();
                        let staging = staging.clone();
                        let client = client.clone();
                        let progress = progress.clone();
                        let progress_counter = progress_counter.clone();
                        let fpi = fpi.clone();
                        let krpatchzp = krpatchz_path.clone();
                        let game_path = game_path.clone();
                        let patch_lock = patch_lock.clone();
                        async move {
                            let progress_counter = progress_counter.clone();
                            let progress = progress.clone();
                            let client = client.clone();
                            let krpatchz_path = krpatchzp.clone();
                            let patch_lock = patch_lock.clone();

                            let spc = staging.clone();
                            let cb = chunk_base.clone();
                            let fpi = fpi.clone();
                            let game_path = game_path.clone();

                            let filen = ff.dest.strip_prefix("/").unwrap_or(&ff.dest);
                            let output_path = spc.join(filen);
                            let valid = validate_checksum(output_path.as_path(), ff.clone().md5.to_ascii_lowercase()).await;

                            if output_path.exists() && valid {
                                progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                progress(processed, total_bytes);
                                return;
                            } else {
                                if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                            }

                            let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                            let dlf = dl.download(output_path.clone(), |_, _| {}).await;

                            if dlf.is_ok() && output_path.exists() {
                                let r1 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                                if r1 {
                                    if filen.ends_with(".krdiff") {
                                        let _ = patch_lock.lock().await;
                                        let diffp = spc.join(fpi.first().unwrap().clone().dest);
                                        let stringed = diffp.to_str().unwrap().to_string();
                                        if let Err(e) = krpatchz(krpatchz_path.to_owned(), &game_path, &stringed) { eprintln!("Failed to krdiff with error: {}", e)}
                                        if diffp.exists() { tokio::fs::remove_file(diffp).await.unwrap(); }
                                    }
                                    progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress(processed, total_bytes);
                                } else {
                                    // Retry to download if we fail
                                    let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                                    let dlf1 = dl.download(output_path.clone(), |_, _| {}).await;
                                    if dlf1.is_ok() {
                                        let r2 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                                        if r2 {
                                            if filen.ends_with(".krdiff") {
                                                let _ = patch_lock.lock().await;
                                                let diffp = spc.join(fpi.first().unwrap().clone().dest);
                                                let stringed = diffp.to_str().unwrap().to_string();
                                                if let Err(e) = krpatchz(krpatchz_path.to_owned(), &game_path, &stringed) { eprintln!("Failed to krdiff with error: {}", e)}
                                                if diffp.exists() { tokio::fs::remove_file(diffp).await.unwrap(); }
                                            }
                                            progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                            let processed = progress_counter.load(Ordering::SeqCst);
                                            progress(processed, total_bytes);
                                        }
                                    }
                                }
                            }
                        }
                    })).buffer_unordered(8).collect::<Vec<()>>();
                    file_tasks.await;
                    // All files are complete make sure we report done just in case
                    progress(total_bytes, total_bytes);
                    let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                    if moved.is_ok() {
                        tokio::fs::remove_dir_all(p.as_path()).await.unwrap();
                        if files.delete_files.is_some() {
                            let dfl = files.delete_files.unwrap();
                            if !dfl.is_empty() { for df in dfl { let dfp = mainp.join(&df); if dfp.exists() { tokio::fs::remove_file(&dfp).await.unwrap(); } } }
                        }
                    }
                    true
                }
            } else {
                // No krdiff download every resource available
                let file_tasks = futures::stream::iter(files.resource.into_iter().map(|ff| {
                    let chunk_base = chunk_base.clone();
                    let staging = staging.clone();
                    let client = client.clone();
                    let progress = progress.clone();
                    let progress_counter = progress_counter.clone();
                    async move {
                        let progress_counter = progress_counter.clone();
                        let progress = progress.clone();
                        let client = client.clone();

                        let spc = staging.clone();
                        let cb = chunk_base.clone();

                        let filen = ff.dest.strip_prefix("/").unwrap_or(&ff.dest);
                        let output_path = spc.join(filen);
                        let valid = validate_checksum(output_path.as_path(), ff.clone().md5.to_ascii_lowercase()).await;

                        if output_path.exists() && valid {
                            progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                            let processed = progress_counter.load(Ordering::SeqCst);
                            progress(processed, total_bytes);
                            return;
                        } else {
                            if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                        }

                        let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                        let dlf = dl.download(output_path.clone(), |_, _| {}).await;

                        if dlf.is_ok() && output_path.exists() {
                            let r1 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                            if r1 {
                                progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                progress(processed, total_bytes);
                            } else {
                                // Retry to download if we fail
                                let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                                let dlf1 = dl.download(output_path.clone(), |_, _| {}).await;
                                if dlf1.is_ok() {
                                    let r2 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                                    if r2 {
                                        progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                        let processed = progress_counter.load(Ordering::SeqCst);
                                        progress(processed, total_bytes);
                                    }
                                }
                            }
                        }
                    }
                })).buffer_unordered(8).collect::<Vec<()>>();
                file_tasks.await;
                // All files are complete make sure we report done just in case
                progress(total_bytes, total_bytes);
                let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                if moved.is_ok() {
                    tokio::fs::remove_dir_all(p.as_path()).await.unwrap();
                    if files.delete_files.is_some() {
                        let dfl = files.delete_files.unwrap();
                        if !dfl.is_empty() { for df in dfl { let dfp = mainp.join(&df); if dfp.exists() { tokio::fs::remove_file(&dfp).await.unwrap(); } } }
                    }
                }
                true
            }
        } else {
            false
        }
    }

    async fn repair_game<F>(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str());
        let p = mainp.to_path_buf().join("repairing");
        let dlptch = mainp.join("patching");
        let dlp = mainp.join("downloading");

        if dlptch.exists() { tokio::fs::remove_dir_all(&dlptch).await.unwrap(); }
        if dlp.exists() { tokio::fs::remove_dir_all(&dlp).await.unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client().await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
        let dll = dl.download(p.clone().join("manifest.json"), |_, _| {}).await;

        if dll.is_ok() {
            let mut f = tokio::fs::File::open(p.join("manifest.json").as_path()).await.unwrap();
            let mut reader = String::new();
            f.read_to_string(&mut reader).await.unwrap();
            let files: KuroIndex = serde_json::from_str(&reader).unwrap();

            let total_bytes: u64 = files.resource.iter().map(|f| f.size).sum();
            let progress_counter = Arc::new(AtomicU64::new(0));
            let progress = Arc::new(progress);

            let file_tasks = futures::stream::iter(files.resource.into_iter().map(|ff| {
                let chunk_base = chunk_base.clone();
                let client = client.clone();
                let progress = progress.clone();
                let progress_counter = progress_counter.clone();
                async move {
                    let mainp = mainp.join(ff.dest.strip_prefix("/").unwrap_or(&ff.dest));

                    let cb = chunk_base.clone();
                    let output_path = mainp.clone();
                    let progress = progress.clone();
                    let progress_counter = progress_counter.clone();
                    let client = client.clone();

                    if output_path.exists() {
                        let valid = if is_fast { output_path.metadata().unwrap().len() == ff.size } else { validate_checksum(&output_path, ff.md5.clone()).await };
                        if !valid {
                            if output_path.exists() {
                                tokio::fs::remove_file(&output_path).await.unwrap();
                                if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                            } else {
                                if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                            }

                            let filen = ff.dest.strip_prefix("/").unwrap_or(&ff.dest);
                            let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                            let dlf = dl.download(output_path.clone(), |_, _| {}).await;

                            if dlf.is_ok() && output_path.exists() {
                                let r1 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                                if r1 {
                                    progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress(processed, total_bytes);
                                } else {
                                    // Retry to download if we fail
                                    let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                                    let dlf1 = dl.download(output_path.clone(), |_, _| {}).await;
                                    if dlf1.is_ok() {
                                        let r2 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                                        if r2 {
                                            progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                            let processed = progress_counter.load(Ordering::SeqCst);
                                            progress(processed, total_bytes);
                                        }
                                    }

                                }
                            }
                        } else {
                            progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                            let processed = progress_counter.load(Ordering::SeqCst);
                            progress(processed, total_bytes);
                        }
                    } else {
                        if output_path.exists() {
                            tokio::fs::remove_file(&output_path).await.unwrap();
                            if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                        } else {
                            if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                        }

                        let filen = ff.dest.strip_prefix("/").unwrap_or(&ff.dest);
                        let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                        let dlf = dl.download(output_path.clone(), |_, _| {}).await;

                        if dlf.is_ok() && output_path.exists() {
                            let r1 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                            if r1 {
                                progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                progress(processed, total_bytes);
                            } else {
                                // Retry to download if we fail
                                let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{filen}").to_string()).await.unwrap();
                                let dlf1 = dl.download(output_path.clone(), |_, _| {}).await;
                                if dlf1.is_ok() {
                                    let r2 = validate_checksum(output_path.as_path(), ff.md5.to_ascii_lowercase()).await;
                                    if r2 {
                                        progress_counter.fetch_add(ff.size, Ordering::SeqCst);
                                        let processed = progress_counter.load(Ordering::SeqCst);
                                        progress(processed, total_bytes);
                                    }
                                }
                            }
                        }
                    }
                }
            })).buffer_unordered(8).collect::<Vec<()>>();
            file_tasks.await;
            // All files are complete make sure we report done just in case
            progress(total_bytes, total_bytes);
            if p.exists() { tokio::fs::remove_dir_all(p.as_path()).await.unwrap(); }
            true
        } else {
            false
        }
    }

    async fn preload<F>(manifest: String, version: String, chunk_base: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() || version.is_empty() { return false; }
        progress(1000, 1000);
        true
    }
}