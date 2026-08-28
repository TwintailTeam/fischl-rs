use crate::download::game::{Game, Yostar};
use crate::utils::downloader::AsyncDownloader;
use crate::utils::{FailedChunk,KuroResource,SpeedTracker,YostarIndex,move_all,validate_checksum};
use crossbeam_deque::{Injector,Steal,Worker};
use tokio::io::AsyncReadExt;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
use std::time::Duration;

// Yostar manifests list every file with a leading slash, a crc64 hash and a stringified size, flatten them into the same task shape the kuro workers use
fn flatten_index(index: YostarIndex) -> Vec<KuroResource> {
    index.file.into_iter().map(|f| KuroResource {
        dest: f.path.trim_start_matches('/').to_string(),
        md5: format!("crc64:{}", f.hash),
        sample_hash: None,
        size: f.size.parse::<u64>().unwrap_or(0),
        from_folder: None,
        chunk_infos: None
    }).collect()
}

impl Yostar for Game {
    async fn download<F>(manifest: String, base_url: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || base_url.is_empty() { return false; }

        let p = Path::new(game_path.as_str()).to_path_buf();
        let dlp = p.join("downloading");
        let dlr = p.join("repairing");
        let dlptch = p.join("patching");

        if dlr.exists() { tokio::fs::remove_dir_all(&dlr).await.unwrap(); }
        if dlptch.exists() { tokio::fs::remove_dir_all(&dlptch).await.unwrap(); }

        let manifest_file = dlp.clone().join("manifest.json");
        if manifest_file.exists() { let _ = tokio::fs::remove_file(manifest_file.clone()).await; }
        let client = Arc::new(AsyncDownloader::setup_client(false).await);
        let dl_result = AsyncDownloader::new(client.clone(), manifest).await;
        if dl_result.is_err() { eprintln!("Failed to connect for manifest download: {:?}", dl_result.err()); return false; }
        let mut dl = dl_result.unwrap().with_cancel_token(cancel_token.clone());
        let dll = dl.download(manifest_file.clone(), |_, _, _, _| {}).await;

        if dll.is_ok() {
            let mut f = tokio::fs::File::open(manifest_file.clone().as_path()).await.unwrap();
            let mut reader = String::new();
            f.read_to_string(&mut reader).await.unwrap();
            let actual_files: serde_json::error::Result<YostarIndex> = serde_json::from_str(&reader);
            if actual_files.is_err() { return false; }
            let files = flatten_index(actual_files.unwrap());

            let staging = dlp.join("staging");
            if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }

            let total_bytes: u64 = files.iter().map(|f| f.size).sum();
            let download_counter = Arc::new(AtomicU64::new(0));
            let install_counter = Arc::new(AtomicU64::new(0));
            let active_verifications = Arc::new(AtomicU64::new(0));
            let active_downloads = Arc::new(AtomicU64::new(0));
            let net_tracker = Arc::new(SpeedTracker::new());
            let disk_tracker = Arc::new(SpeedTracker::new());
            let progress = Arc::new(progress);
            let failed_chunks: Arc<std::sync::Mutex<Vec<FailedChunk>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

            let monitor_handle = tokio::spawn({
                let download_counter = download_counter.clone();
                let install_counter = install_counter.clone();
                let active_verifications = active_verifications.clone();
                let active_downloads = active_downloads.clone();
                let net_tracker = net_tracker.clone();
                let disk_tracker = disk_tracker.clone();
                let progress = progress.clone();
                async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        let on_disk = download_counter.load(Ordering::SeqCst);
                        let active_dl = net_tracker.get_total();
                        let download_current = on_disk.saturating_add(active_dl).min(total_bytes);
                        let install_current = install_counter.load(Ordering::SeqCst);
                        let net_speed = net_tracker.update();
                        let disk_speed = disk_tracker.update();
                        let verifying = active_verifications.load(Ordering::SeqCst);
                        let downloading = active_downloads.load(Ordering::SeqCst);
                        let phase = if downloading > 0 { 2 } else if verifying > 0 { 4 } else { 0 };
                        progress(download_current, total_bytes, install_current, total_bytes, net_speed, disk_speed, phase);
                    }
                }
            });

            // Start of download code
            let injector = Arc::new(Injector::<KuroResource>::new());
            let mut workers = Vec::new();
            let mut stealers_list = Vec::new();
            for _ in 0..6 { let w = Worker::<KuroResource>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
            let stealers = Arc::new(stealers_list);
            for task in files.into_iter() { injector.push(task); }
            let file_sem = Arc::new(tokio::sync::Semaphore::new(6));

            // Spawn worker tasks
            let mut handles = Vec::with_capacity(6);
            for _i in 0..workers.len() {
                let local_worker = workers.pop().unwrap();
                let stealers = stealers.clone();
                let injector = injector.clone();
                let file_sem = file_sem.clone();

                let stealers = stealers.clone();
                let download_counter = download_counter.clone();
                let install_counter = install_counter.clone();
                let active_verifications = active_verifications.clone();
                let active_downloads = active_downloads.clone();
                let net_tracker = net_tracker.clone();
                let disk_tracker = disk_tracker.clone();
                let chunk_base = base_url.clone();
                let staging = staging.clone();
                let client = client.clone();
                let cancel_token = cancel_token.clone();
                let verified_files = verified_files.clone();
                let failed_chunks = failed_chunks.clone();

                let mut retry_tasks = Vec::new();
                let handle = tokio::task::spawn(async move {
                    loop {
                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { break; } }
                        let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                            for s in stealers.iter() { if let Steal::Success(t) = s.steal() { return Some(t); } }
                            None
                        });
                        let Some(chunk_task) = job else { break; };
                        let permit = file_sem.clone().acquire_owned().await.unwrap();

                        let ct = tokio::spawn({
                            let download_counter = download_counter.clone();
                            let install_counter = install_counter.clone();
                            let active_verifications = active_verifications.clone();
                            let active_downloads = active_downloads.clone();
                            let net_tracker = net_tracker.clone();
                            let disk_tracker = disk_tracker.clone();
                            let chunk_base = chunk_base.clone();
                            let staging = staging.clone();
                            let client = client.clone();
                            let cancel_token = cancel_token.clone();
                            let verified_files = verified_files.clone();
                            let failed_chunks = failed_chunks.clone();
                            async move {
                                if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { drop(permit); return; } }
                                let staging_dir = staging.join(chunk_task.dest.clone());

                                let mut already_verified = false;
                                if let Some(vf) = &verified_files {
                                    let v = vf.lock().unwrap();
                                    if v.contains(&chunk_task.dest) { already_verified = true; }
                                }

                                // Count existing bytes toward download progress BEFORE validation
                                // Prevents snap-back on resume while large files are still being checksummed
                                let existing_size = if staging_dir.exists() { staging_dir.metadata().map(|m| m.len().min(chunk_task.size)).unwrap_or(0) } else { 0 };
                                if existing_size > 0 { download_counter.fetch_add(existing_size, Ordering::SeqCst); }

                                // Verification phase - checking if file exists and is valid
                                active_verifications.fetch_add(1, Ordering::SeqCst);
                                let cvalid = if already_verified { true } else { validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await };
                                active_verifications.fetch_sub(1, Ordering::SeqCst);

                                if staging_dir.exists() && cvalid {
                                    if !already_verified {
                                        if let Some(vf) = &verified_files {
                                            let mut v = vf.lock().unwrap();
                                            v.insert(chunk_task.dest.clone());
                                        }
                                    }
                                    // download_counter already has existing_size, add any remainder
                                    let remaining = chunk_task.size.saturating_sub(existing_size);
                                    if remaining > 0 { download_counter.fetch_add(remaining, Ordering::SeqCst); }
                                    install_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    return;
                                }

                                let pn = chunk_task.dest.clone();
                                let url = format!("{chunk_base}/{pn}");
                                let mut last_error = String::new();
                                let mut success = false;
                                let mut cancelled = false;

                                active_downloads.fetch_add(1, Ordering::SeqCst);
                                // Try up to 3 times
                                for attempt in 0..3 {
                                    if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { cancelled = true; break; } }
                                    let dl_result = AsyncDownloader::new(client.clone(), url.clone()).await;
                                    if let Err(e) = dl_result { last_error = e.to_string(); continue; }
                                    // Graceful pause: let an active file finish before pausing.
                                    let mut dl = dl_result.unwrap();
                                    let net_t = net_tracker.clone();
                                    let disk_t = disk_tracker.clone();
                                    // Start from current file size so net_tracker only tracks NEW bytes (no overlap with download_counter)
                                    let cur_size = staging_dir.metadata().map(|m| m.len()).unwrap_or(0);
                                    let mut last_written = cur_size;
                                    let dlf = dl.download(staging_dir.clone(), move |current, _total, _ns, _ds| { let diff = current.saturating_sub(last_written); if diff > 0 { net_t.add_bytes(diff); disk_t.add_bytes(diff); } last_written = current; }).await;
                                    if let Err(e) = &dlf {
                                        last_error = e.to_string();
                                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { cancelled = true; break; } }
                                        continue;
                                    }
                                    let cvalid = validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
                                    if cvalid {
                                        if !already_verified { if let Some(vf) = &verified_files { vf.lock().unwrap().insert(chunk_task.dest.clone()); } }
                                        install_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                        success = true;
                                        break;
                                    } else { last_error = format!("Checksum mismatch on attempt {}", attempt + 1); }
                                }
                                active_downloads.fetch_sub(1, Ordering::SeqCst);

                                if !success && !cancelled {
                                    eprintln!("Failed to download file {} after 3 retries: {}", pn, last_error);
                                    failed_chunks.lock().unwrap().push(FailedChunk { file_name: pn.clone(), chunk_name: pn.clone(), error: last_error });
                                }
                                drop(permit);
                            }
                        }); // end task
                        retry_tasks.push(ct);
                    }
                    // Graceful pause: wait for in-flight file tasks to finish.
                    for t in retry_tasks { let _ = t.await; }
                });
                handles.push(handle);
            }
            for handle in handles { let _ = handle.await; }

            if let Some(token) = &cancel_token {
                if token.load(Ordering::Relaxed) {
                    monitor_handle.abort();
                    return false;
                }
            }
            monitor_handle.abort();

            // Report failed chunks
            let failures = failed_chunks.lock().unwrap();
            if !failures.is_empty() {
                eprintln!("\n=== Download completed with {} failed file(s) ===", failures.len());
                for fc in failures.iter() { eprintln!("  - File: {}, Error: {}", fc.file_name, fc.error); }
                eprintln!("Please run 'Game Repair' after this download completes to fix affected files.\n");
            }
            drop(failures);

            // Download complete, now move files (phase 5 = moving)
            progress(total_bytes, total_bytes, total_bytes, total_bytes, 0, 0, 5);
            let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
            if moved.is_ok() { let _ = tokio::fs::remove_dir_all(dlp.as_path()).await; }
            true
        } else { false }
    }

    async fn patch<F>(_manifest: String, _base_url: String, _game_path: String, _preloaded: bool, _progress: F, _cancel_token: Option<Arc<AtomicBool>>, _verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        // Yostar ships no diffs, updates are a plain download against the new manifest
        false
    }

    async fn repair_game<F>(manifest: String, base_url: String, game_path: String, is_fast: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || base_url.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.to_path_buf().join("repairing");
        let dlptch = mainp.join("patching");
        let dlp = mainp.join("downloading");

        if dlptch.exists() { let _ = tokio::fs::remove_dir_all(&dlptch).await; }
        if dlp.exists() { let _ = tokio::fs::remove_dir_all(&dlp).await; }

        let manifest_file = p.clone().join("manifest.json");
        if manifest_file.exists() { let _ = tokio::fs::remove_file(manifest_file.clone()).await; }
        let client = Arc::new(AsyncDownloader::setup_client(false).await);
        let dl_result = AsyncDownloader::new(client.clone(), manifest).await;
        if dl_result.is_err() { eprintln!("Failed to connect for repair manifest: {:?}", dl_result.err()); return false; }
        let mut dl = dl_result.unwrap().with_cancel_token(cancel_token.clone());
        let dll = dl.download(manifest_file.clone(), |_, _, _, _| {}).await;

        if dll.is_ok() {
            let mut f = tokio::fs::File::open(manifest_file.clone().as_path()).await.unwrap();
            let mut reader = String::new();
            f.read_to_string(&mut reader).await.unwrap();
            let actual_files: serde_json::error::Result<YostarIndex> = serde_json::from_str(&reader);
            if actual_files.is_err() { return false; }
            let files = flatten_index(actual_files.unwrap());

            let total_bytes: u64 = files.iter().map(|f| f.size).sum();
            let download_counter = Arc::new(AtomicU64::new(0));
            let install_counter = Arc::new(AtomicU64::new(0));
            let active_verifications = Arc::new(AtomicU64::new(0));
            let active_downloads = Arc::new(AtomicU64::new(0));
            let net_tracker = Arc::new(SpeedTracker::new());
            let disk_tracker = Arc::new(SpeedTracker::new());
            let progress = Arc::new(progress);
            let failed_chunks: Arc<std::sync::Mutex<Vec<FailedChunk>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

            let monitor_handle = tokio::spawn({
                let download_counter = download_counter.clone();
                let install_counter = install_counter.clone();
                let active_verifications = active_verifications.clone();
                let active_downloads = active_downloads.clone();
                let net_tracker = net_tracker.clone();
                let disk_tracker = disk_tracker.clone();
                let progress = progress.clone();
                async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        let on_disk = download_counter.load(Ordering::SeqCst);
                        let active_dl = net_tracker.get_total();
                        let download_current = on_disk.saturating_add(active_dl).min(total_bytes);
                        let install_current = install_counter.load(Ordering::SeqCst);
                        let net_speed = net_tracker.update();
                        let disk_speed = disk_tracker.update();
                        let verifying = active_verifications.load(Ordering::SeqCst);
                        let downloading = active_downloads.load(Ordering::SeqCst);
                        let phase = if downloading > 0 { 2 } else if verifying > 0 { 4 } else { 0 };
                        progress(download_current, total_bytes, install_current, total_bytes, net_speed, disk_speed, phase);
                    }
                }
            });

            // Start of download code
            let injector = Arc::new(Injector::<KuroResource>::new());
            let mut workers = Vec::new();
            let mut stealers_list = Vec::new();
            for _ in 0..6 { let w = Worker::<KuroResource>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
            let stealers = Arc::new(stealers_list);
            for task in files.into_iter() { injector.push(task); }
            let file_sem = Arc::new(tokio::sync::Semaphore::new(6));

            // Spawn worker tasks
            let mut handles = Vec::with_capacity(6);
            for _i in 0..workers.len() {
                let local_worker = workers.pop().unwrap();
                let stealers = stealers.clone();
                let injector = injector.clone();
                let file_sem = file_sem.clone();

                let stealers = stealers.clone();
                let download_counter = download_counter.clone();
                let install_counter = install_counter.clone();
                let active_verifications = active_verifications.clone();
                let active_downloads = active_downloads.clone();
                let net_tracker = net_tracker.clone();
                let disk_tracker = disk_tracker.clone();
                let chunk_base = base_url.clone();
                let staging = mainp.clone();
                let client = client.clone();
                let failed_chunks = failed_chunks.clone();
                let cancel_token = cancel_token.clone();
                let verified_files = verified_files.clone();

                let mut retry_tasks = Vec::new();
                let handle = tokio::task::spawn(async move {
                    loop {
                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { break; } }
                        let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                            for s in stealers.iter() { if let Steal::Success(t) = s.steal() { return Some(t); } }
                            None
                        });
                        let Some(chunk_task) = job else { break; };
                        let permit = file_sem.clone().acquire_owned().await.unwrap();

                        let ct = tokio::spawn({
                            let download_counter = download_counter.clone();
                            let install_counter = install_counter.clone();
                            let active_verifications = active_verifications.clone();
                            let active_downloads = active_downloads.clone();
                            let net_tracker = net_tracker.clone();
                            let disk_tracker = disk_tracker.clone();
                            let chunk_base = chunk_base.clone();
                            let staging = staging.clone();
                            let client = client.clone();
                            let failed_chunks = failed_chunks.clone();
                            let cancel_token = cancel_token.clone();
                            let verified_files = verified_files.clone();
                            async move {
                                if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { drop(permit); return; } }
                                let staging_dir = staging.join(chunk_task.dest.clone());

                                let mut already_verified = false;
                                if let Some(vf) = &verified_files {
                                    let v = vf.lock().unwrap();
                                    if v.contains(&chunk_task.dest) { already_verified = true; }
                                }

                                // Verification phase - checking if file exists and is valid
                                active_verifications.fetch_add(1, Ordering::SeqCst);
                                let cvalid = if already_verified { true } else if is_fast { staging_dir.metadata().map(|m| m.len() == chunk_task.size).unwrap_or(false) } else { validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await };
                                active_verifications.fetch_sub(1, Ordering::SeqCst);

                                if staging_dir.exists() && cvalid {
                                    if !already_verified {
                                        if let Some(vf) = &verified_files {
                                            let mut v = vf.lock().unwrap();
                                            v.insert(chunk_task.dest.clone());
                                        }
                                    }
                                    download_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    install_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    return;
                                }

                                // File failed validation - delete corrupted file so AsyncDownloader starts fresh
                                // (otherwise it sees matching file size and skips the download)
                                if staging_dir.exists() { let _ = tokio::fs::remove_file(&staging_dir).await; }

                                let pn = chunk_task.dest.clone();
                                let url = format!("{chunk_base}/{pn}");
                                let mut last_error = String::new();
                                let mut success = false;
                                let mut cancelled = false;

                                active_downloads.fetch_add(1, Ordering::SeqCst);
                                for _attempt in 0..3 {
                                    if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { cancelled = true; break; } }
                                    let dl_result = AsyncDownloader::new(client.clone(), url.clone()).await;
                                    if let Err(e) = dl_result { last_error = e.to_string(); continue; }
                                    let mut dl = dl_result.unwrap().with_cancel_token(cancel_token.clone());
                                    let net_t = net_tracker.clone();
                                    let disk_t = disk_tracker.clone();
                                    let cur_size = staging_dir.metadata().map(|m| m.len()).unwrap_or(0);
                                    let mut last_written = cur_size;
                                    let dlf = dl.download(staging_dir.clone(), move |current, _total, _ns, _ds| { let diff = current.saturating_sub(last_written); if diff > 0 { net_t.add_bytes(diff); disk_t.add_bytes(diff); } last_written = current; }).await;
                                    if let Err(e) = &dlf {
                                        last_error = e.to_string();
                                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { cancelled = true; break; } }
                                        continue;
                                    }
                                    let cvalid = if is_fast { staging_dir.metadata().map(|m| m.len() == chunk_task.size).unwrap_or(false) } else { validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await };
                                    if cvalid {
                                        if !already_verified { if let Some(vf) = &verified_files { vf.lock().unwrap().insert(chunk_task.dest.clone()); } }
                                        install_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                        success = true;
                                        break;
                                    } else { last_error = "Checksum mismatch".to_string(); }
                                }
                                active_downloads.fetch_sub(1, Ordering::SeqCst);

                                if !success && !cancelled {
                                    eprintln!("Failed to repair file {} after 3 retries: {}", pn, last_error);
                                    failed_chunks.lock().unwrap().push(FailedChunk { file_name: pn.clone(), chunk_name: pn.clone(), error: last_error });
                                }
                                drop(permit);
                            }
                        }); // end task
                        retry_tasks.push(ct);
                    }
                    // If cancelled, abort all spawned tasks instead of waiting
                    if let Some(token) = &cancel_token {
                        if token.load(Ordering::Relaxed) {
                            for t in retry_tasks { t.abort(); }
                            return;
                        }
                    }
                    for t in retry_tasks { let _ = t.await; }
                });
                handles.push(handle);
            }
            for handle in handles { let _ = handle.await; }

            if let Some(token) = &cancel_token {
                if token.load(Ordering::Relaxed) {
                    monitor_handle.abort();
                    return false;
                }
            }
            monitor_handle.abort();

            // Report failed chunks
            let failures = failed_chunks.lock().unwrap();
            if !failures.is_empty() {
                eprintln!("\n=== Repair completed with {} failed file(s) ===", failures.len());
                for fc in failures.iter() { eprintln!("  - File: {}, Error: {}", fc.file_name, fc.error); }
                eprintln!("Some files could not be repaired. Please try again or reinstall the game.\n");
            }
            drop(failures);

            // Repair complete
            progress(total_bytes, total_bytes, total_bytes, total_bytes, 0, 0, 0);
            if p.exists() { let _ = tokio::fs::remove_dir_all(p.as_path()).await; }
            true
        } else { false }
    }

    async fn preload<F>(_manifest: String, _base_url: String, _game_path: String, _progress: F, _cancel_token: Option<Arc<AtomicBool>>, _verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        // Yostar exposes no predownload, the manifest only ever points at the live version
        false
    }
}
