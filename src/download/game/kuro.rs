use crate::download::game::{Game, Kuro};
use crate::utils::downloader::AsyncDownloader;
use crate::utils::{FailedChunk,KuroIndex,KuroResource,SpeedTracker,extract_archive_with_progress,move_all,validate_checksum};
use crossbeam_deque::{Injector,Steal,Worker};
use hdiffpatch_rs::patchers::KrDiff;
use tokio::io::AsyncReadExt;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
use std::time::Duration;

impl Kuro for Game {
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
            let actual_files: serde_json::error::Result<KuroIndex> = serde_json::from_str(&reader);
            if actual_files.is_err() { return false; }
            let files = actual_files.unwrap();

            let staging = dlp.join("staging");
            if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }

            let total_bytes: u64 = files.resource.iter().map(|f| f.size).sum();
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
            for task in files.resource.into_iter() { injector.push(task); }
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

    async fn patch<F>(manifest: String, base_resources: String, base_zip: String, game_path: String, preloaded: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>,verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || base_resources.is_empty() || base_zip.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str());
        let p = mainp.join("patching");
        let dlp = mainp.join("downloading");
        let dlr = mainp.join("repairing");

        if dlr.exists() { let _ = tokio::fs::remove_dir_all(&dlr).await; }
        if dlp.exists() { let _ = tokio::fs::remove_dir_all(&dlp).await; }

        let manifest_file = p.clone().join("manifest.json");
        if manifest_file.exists() { let _ = tokio::fs::remove_file(manifest_file.clone()).await; }
        let client = Arc::new(AsyncDownloader::setup_client(false).await);
        let dl_result = AsyncDownloader::new(client.clone(), manifest).await;
        if dl_result.is_err() { eprintln!("Failed to connect for patch manifest: {:?}", dl_result.err()); return false; }
        let mut dl = dl_result.unwrap().with_cancel_token(cancel_token.clone());
        let dll = dl.download(manifest_file.clone(), |_, _, _, _| {}).await;

        if dll.is_ok() {
            let mut f = tokio::fs::File::open(manifest_file.clone().as_path()).await.unwrap();
            let mut reader = String::new();
            f.read_to_string(&mut reader).await.unwrap();
            let actual_files: serde_json::error::Result<KuroIndex> = serde_json::from_str(&reader);
            if actual_files.is_err() { return false; }
            let files = actual_files.unwrap();

            let staging = p.join("staging");
            if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }

            let progress = Arc::new(progress);

            if preloaded {
                // Preloaded: files already in staging, just extract/apply then move
                let install_total: u64 = {
                    let mut t = 0u64;
                    if let Some(zips) = &files.zip_infos { for z in zips { t += z.entries.iter().map(|e| e.size).sum::<u64>(); } }
                    if let Some(diffs) = &files.patch_infos { for d in diffs { t += d.entries.iter().map(|e| e.size).sum::<u64>(); } }
                    if let Some(diffs) = &files.group_infos { for d in diffs { t += d.dst_files.iter().map(|f| f.size).sum::<u64>(); } }
                    t
                };
                let download_total: u64 = files.resource.iter().map(|f| f.size).sum();
                let install_counter = Arc::new(AtomicU64::new(0));

                // Pre-credit diffs already applied (file is deleted after apply, so absence = done)
                if let Some(diffs) = &files.patch_infos {
                    for d in diffs {
                        if !staging.join(d.dest.clone()).exists() { install_counter.fetch_add(d.entries.iter().map(|e| e.size).sum::<u64>(), Ordering::SeqCst); }
                    }
                }
                if let Some(diffs) = &files.group_infos {
                    for d in diffs {
                        if !staging.join(d.dest.clone()).exists() { install_counter.fetch_add(d.dst_files.iter().map(|f| f.size).sum::<u64>(), Ordering::SeqCst); }
                    }
                }

                let monitor_handle = tokio::spawn({
                    let install_counter = install_counter.clone();
                    let progress = progress.clone();
                    async move {
                        loop {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            let install_current = install_counter.load(Ordering::SeqCst);
                            progress(download_total, download_total, install_current, install_total, 0, 0, 3);
                        }
                    }
                });

                // Extract zips (PGR) — stream bytes delta into install_counter via callback
                if let Some(zips) = files.zip_infos {
                    for z in zips {
                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { monitor_handle.abort(); return false; } }
                        let zp = staging.join(z.dest.clone());
                        if zp.exists() {
                            let zip_total: u64 = z.entries.iter().map(|e| e.size).sum();
                            let before = install_counter.load(Ordering::SeqCst);
                            let last_reported = Arc::new(AtomicU64::new(0));
                            let ic = install_counter.clone();
                            let pc = progress.clone();
                            let it = install_total;
                            let dt = download_total;
                            extract_archive_with_progress(zp.to_str().unwrap().to_string(), staging.to_str().unwrap().to_string(), false, move |done, _| {
                                let prev = last_reported.load(Ordering::Relaxed);
                                let delta = done.saturating_sub(prev);
                                if delta > 0 { last_reported.store(done, Ordering::Relaxed); let v = ic.fetch_add(delta, Ordering::SeqCst) + delta; pc(dt, dt, v, it, 0, 0, 3); }
                            });
                            // Credit any final bytes below the callback's report threshold
                            let credited = install_counter.load(Ordering::SeqCst).saturating_sub(before);
                            let remainder = zip_total.saturating_sub(credited);
                            if remainder > 0 { let v = install_counter.fetch_add(remainder, Ordering::SeqCst) + remainder; progress(download_total, download_total, v, install_total, 0, 0, 3); }
                        }
                    }
                }
                // Apply krdiffs (Wuwa) — skip if already applied (file deleted), credit entries sum after apply
                if let Some(diffs) = files.patch_infos {
                    for d in diffs {
                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { monitor_handle.abort(); return false; } }
                        let diffp = staging.join(d.dest.clone());
                        if !diffp.exists() { continue; } // already applied and pre-credited
                        let stgs = staging.to_str().unwrap().to_string();
                        let stringed = diffp.to_str().unwrap().to_string();
                        let mut krdiff = KrDiff::new(game_path.clone(), stringed, stgs);
                        let krd = krdiff.apply();
                        if krd { let fsize: u64 = d.entries.iter().map(|e| e.size).sum(); let v = install_counter.fetch_add(fsize, Ordering::SeqCst) + fsize; progress(download_total, download_total, v, install_total, 0, 0, 3); } else { eprintln!("Failed to apply krdiff!") }
                        if diffp.exists() { tokio::fs::remove_file(diffp).await.unwrap(); }
                    }
                }
                // Apply krpdiffs (Wuwa) — skip if already applied (file deleted), credit entries sum after apply
                if let Some(diffs) = files.group_infos {
                    for d in diffs {
                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { monitor_handle.abort(); return false; } }
                        let diffp = staging.join(d.dest.clone());
                        if !diffp.exists() { continue; } // already applied and pre-credited
                        let stgs = staging.to_str().unwrap().to_string();
                        let stringed = diffp.to_str().unwrap().to_string();
                        let mut krdiff = KrDiff::new(game_path.clone(), stringed, stgs);
                        let krd = krdiff.apply();
                        if krd { let fsize: u64 = d.dst_files.iter().map(|f| f.size).sum(); let v = install_counter.fetch_add(fsize, Ordering::SeqCst) + fsize; progress(download_total, download_total, v, install_total, 0, 0, 3); } else { eprintln!("Failed to apply krpdiff!") }
                        if diffp.exists() { tokio::fs::remove_file(diffp).await.unwrap(); }
                    }
                }

                monitor_handle.abort();
                // Move phase (phase 5)
                progress(download_total, download_total, install_total, install_total, 0, 0, 5);
                let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                if moved.is_ok() {
                    let _ = tokio::fs::remove_dir_all(p.as_path()).await;
                    if let Some(dfl) = files.delete_files {
                        if !dfl.is_empty() { for df in dfl { let dfp = mainp.join(&df); if dfp.exists() { let _ = tokio::fs::remove_file(&dfp).await; } } }
                    }
                }
                true
            } else {
                // Non-preloaded: download raw patch files first, then extract/apply, then move
                // Raw files (not zips/diffs) are downloaded and credited to install bar after validation
                let raw_files_total: u64 = files.resource.iter().filter(|r| !r.dest.ends_with(".krzip") && !r.dest.ends_with(".krdiff") && !r.dest.ends_with(".krpdiff")).map(|r| r.size).sum();
                let install_total: u64 = {
                    let mut t = raw_files_total;
                    if let Some(zips) = &files.zip_infos { for z in zips { t += z.entries.iter().map(|e| e.size).sum::<u64>(); } }
                    if let Some(diffs) = &files.patch_infos { for d in diffs { t += d.entries.iter().map(|e| e.size).sum::<u64>(); } }
                    if let Some(diffs) = &files.group_infos { for d in diffs { t += d.dst_files.iter().map(|f| f.size).sum::<u64>(); } }
                    t
                };
                let download_total: u64 = files.resource.iter().map(|f| f.size).sum();
                let download_counter = Arc::new(AtomicU64::new(0));
                let install_counter = Arc::new(AtomicU64::new(0));
                let active_verifications = Arc::new(AtomicU64::new(0));
                let active_downloads = Arc::new(AtomicU64::new(0));
                let net_tracker = Arc::new(SpeedTracker::new());
                let disk_tracker = Arc::new(SpeedTracker::new());
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
                            let download_current = on_disk.saturating_add(active_dl).min(download_total);
                            let install_current = install_counter.load(Ordering::SeqCst);
                            let net_speed = net_tracker.update();
                            let disk_speed = disk_tracker.update();
                            let verifying = active_verifications.load(Ordering::SeqCst);
                            let downloading = active_downloads.load(Ordering::SeqCst);
                            let phase = if downloading > 0 { 2 } else if verifying > 0 { 4 } else { 0 };
                            progress(download_current, download_total, install_current, install_total, net_speed, disk_speed, phase);
                        }
                    }
                });

                let injector = Arc::new(Injector::<KuroResource>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..6 { let w = Worker::<KuroResource>::new_fifo(); stealers_list.push(w.stealer()); workers.push(w); }
                let stealers = Arc::new(stealers_list);
                for task in files.resource.into_iter() { injector.push(task); }
                let file_sem = Arc::new(tokio::sync::Semaphore::new(6));

                let mut handles = Vec::with_capacity(6);
                for _i in 0..workers.len() {
                    let local_worker = workers.pop().unwrap();
                    let stealers = stealers.clone();
                    let injector = injector.clone();
                    let file_sem = file_sem.clone();
                    let download_counter = download_counter.clone();
                    let install_counter = install_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let chunk_res = base_resources.clone();
                    let chunks_zip = base_zip.clone();
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
                                let chunk_res = chunk_res.clone();
                                let chunks_zip = chunks_zip.clone();
                                let staging = staging.clone();
                                let client = client.clone();
                                let cancel_token = cancel_token.clone();
                                let verified_files = verified_files.clone();
                                let failed_chunks = failed_chunks.clone();
                                async move {
                                    if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { drop(permit); return; } }
                                    let staging_dir = staging.join(chunk_task.dest.clone());
                                    let is_raw = !chunk_task.dest.ends_with(".krzip") && !chunk_task.dest.ends_with(".krdiff") && !chunk_task.dest.ends_with(".krpdiff");

                                    let mut already_verified = false;
                                    if let Some(vf) = &verified_files {
                                        let v = vf.lock().unwrap();
                                        if v.contains(&chunk_task.dest) { already_verified = true; }
                                    }

                                    // Pre-credit existing bytes so progress doesn't snap back during resume
                                    let existing_size = if staging_dir.exists() { staging_dir.metadata().map(|m| m.len().min(chunk_task.size)).unwrap_or(0) } else { 0 };
                                    if existing_size > 0 { download_counter.fetch_add(existing_size, Ordering::SeqCst); }

                                    active_verifications.fetch_add(1, Ordering::SeqCst);
                                    let cvalid = if already_verified { true } else { validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await };
                                    active_verifications.fetch_sub(1, Ordering::SeqCst);

                                    if staging_dir.exists() && cvalid {
                                        if !already_verified { if let Some(vf) = &verified_files { vf.lock().unwrap().insert(chunk_task.dest.clone()); } }
                                        let remaining = chunk_task.size.saturating_sub(existing_size);
                                        if remaining > 0 { download_counter.fetch_add(remaining, Ordering::SeqCst); }
                                        // Raw files already staged: credit to install bar
                                        if is_raw { install_counter.fetch_add(chunk_task.size, Ordering::SeqCst); }
                                        return;
                                    }

                                    let pn = chunk_task.dest.clone();
                                    let chunk_base = if !is_raw { chunk_res } else { chunks_zip + "/" };
                                    let url = format!("{chunk_base}{pn}");
                                    let mut last_error = String::new();
                                    let mut success = false;
                                    let mut cancelled = false;

                                    active_downloads.fetch_add(1, Ordering::SeqCst);
                                    for attempt in 0..3 {
                                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { cancelled = true; break; } }
                                        let dl_result = AsyncDownloader::new(client.clone(), url.clone()).await;
                                        if let Err(e) = dl_result { last_error = e.to_string(); continue; }
                                        // Graceful pause: let an active file finish before pausing.
                                        let mut dl = dl_result.unwrap();
                                        let net_t = net_tracker.clone();
                                        let disk_t = disk_tracker.clone();
                                        // Track only NEW bytes from current file size to avoid overlap with download_counter
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
                                            // Raw files downloaded and validated: credit to install bar
                                            if is_raw { install_counter.fetch_add(chunk_task.size, Ordering::SeqCst); }
                                            success = true;
                                            break;
                                        } else { last_error = format!("Checksum mismatch on attempt {}", attempt + 1); }
                                    }
                                    active_downloads.fetch_sub(1, Ordering::SeqCst);

                                    if !success && !cancelled {
                                        eprintln!("Failed to download patch file {} after 3 retries: {}", pn, last_error);
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
                    eprintln!("\n=== Patch download completed with {} failed file(s) ===", failures.len());
                    for fc in failures.iter() { eprintln!("  - File: {}, Error: {}", fc.file_name, fc.error); }
                    eprintln!("Please run 'Game Repair' after this patch download completes to fix affected files.\n");
                }
                drop(failures);

                // Install phase: extract zips (PGR) — stream bytes delta into install_counter via callback
                if let Some(zips) = files.zip_infos {
                    for z in zips {
                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { return false; } }
                        let zp = staging.join(z.dest.clone());
                        if zp.exists() {
                            let zip_total: u64 = z.entries.iter().map(|e| e.size).sum();
                            let before = install_counter.load(Ordering::SeqCst);
                            let last_reported = Arc::new(AtomicU64::new(0));
                            let ic = install_counter.clone();
                            let pc = progress.clone();
                            let it = install_total;
                            let dt = download_total;
                            extract_archive_with_progress(zp.to_str().unwrap().to_string(), staging.to_str().unwrap().to_string(), false, move |done, _| {
                                let prev = last_reported.load(Ordering::Relaxed);
                                let delta = done.saturating_sub(prev);
                                if delta > 0 { last_reported.store(done, Ordering::Relaxed); let v = ic.fetch_add(delta, Ordering::SeqCst) + delta; pc(dt, dt, v, it, 0, 0, 3); }
                            });
                            // Credit any final bytes below the callback's report threshold
                            let credited = install_counter.load(Ordering::SeqCst).saturating_sub(before);
                            let remainder = zip_total.saturating_sub(credited);
                            if remainder > 0 { let v = install_counter.fetch_add(remainder, Ordering::SeqCst) + remainder; progress(download_total, download_total, v, install_total, 0, 0, 3); }
                        }
                    }
                }
                // Apply krdiffs (Wuwa) — credit entries sum after apply
                if let Some(diffs) = files.patch_infos {
                    for d in diffs {
                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { return false; } }
                        let stgs = staging.to_str().unwrap().to_string();
                        let diffp = staging.join(d.dest.clone());
                        let stringed = diffp.to_str().unwrap().to_string();
                        let mut krdiff = KrDiff::new(game_path.clone(), stringed, stgs);
                        let krd = krdiff.apply();
                        if krd { let fsize: u64 = d.entries.iter().map(|e| e.size).sum(); let v = install_counter.fetch_add(fsize, Ordering::SeqCst) + fsize; progress(download_total, download_total, v, install_total, 0, 0, 3); } else { eprintln!("Failed to apply krdiff!") }
                        if diffp.exists() { tokio::fs::remove_file(diffp).await.unwrap(); }
                    }
                }
                // Apply krpdiffs (Wuwa) — same as krdiff, credit entries sum after apply
                if let Some(diffs) = files.group_infos {
                    for d in diffs {
                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { return false; } }
                        let stgs = staging.to_str().unwrap().to_string();
                        let diffp = staging.join(d.dest.clone());
                        let stringed = diffp.to_str().unwrap().to_string();
                        let mut krdiff = KrDiff::new(game_path.clone(), stringed, stgs);
                        let krd = krdiff.apply();
                        if krd { let fsize: u64 = d.dst_files.iter().map(|f| f.size).sum(); let v = install_counter.fetch_add(fsize, Ordering::SeqCst) + fsize; progress(download_total, download_total, v, install_total, 0, 0, 3); } else { eprintln!("Failed to apply krpdiff!") }
                        if diffp.exists() { tokio::fs::remove_file(diffp).await.unwrap(); }
                    }
                }

                // Move phase (phase 5)
                progress(download_total, download_total, install_total, install_total, 0, 0, 5);
                let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                if moved.is_ok() {
                    let _ = tokio::fs::remove_dir_all(p.as_path()).await;
                    if let Some(dfl) = files.delete_files {
                        if !dfl.is_empty() { for df in dfl { let dfp = mainp.join(&df); if dfp.exists() { let _ = tokio::fs::remove_file(&dfp).await; } } }
                    }
                }
                true
            }
        } else { false }
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
            let actual_files: serde_json::error::Result<KuroIndex> = serde_json::from_str(&reader);
            if actual_files.is_err() { return false; }
            let files = actual_files.unwrap();

            let total_bytes: u64 = files.resource.iter().map(|f| f.size).sum();
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
            for task in files.resource.into_iter() { injector.push(task); }
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

    async fn preload<F>(manifest: String, base_resources: String, base_zip: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || base_resources.is_empty() || base_zip.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str());
        let p = mainp.join("patching");
        let dlp = mainp.join("downloading");
        let dlr = mainp.join("repairing");

        if dlr.exists() { let _ = tokio::fs::remove_dir_all(&dlr).await; }
        if dlp.exists() { let _ = tokio::fs::remove_dir_all(&dlp).await; }

        let manifest_file = p.clone().join("manifest.json");
        if manifest_file.exists() { let _ = tokio::fs::remove_file(manifest_file.clone()).await; }
        let client = Arc::new(AsyncDownloader::setup_client(false).await);
        let dl_result = AsyncDownloader::new(client.clone(), manifest).await;
        if dl_result.is_err() { eprintln!("Failed to connect for preload manifest: {:?}", dl_result.err()); return false; }
        let mut dl = dl_result.unwrap().with_cancel_token(cancel_token.clone());
        let dll = dl.download(manifest_file.clone(), |_, _, _, _| {}).await;

        if dll.is_ok() {
            let mut f = tokio::fs::File::open(manifest_file.clone().as_path()).await.unwrap();
            let mut reader = String::new();
            f.read_to_string(&mut reader).await.unwrap();
            let actual_files: serde_json::error::Result<KuroIndex> = serde_json::from_str(&reader);
            if actual_files.is_err() { return false; }
            let files = actual_files.unwrap();

            let staging = p.join("staging");
            if !p.join(".preload").exists() { fs::File::create(p.join(".preload")).unwrap(); }
            if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }

            let download_total: u64 = files.resource.iter().map(|f| f.size).sum();
            let install_total: u64 = download_total;
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
                        let download_current = on_disk.saturating_add(active_dl).min(download_total);
                        let install_current = install_counter.load(Ordering::SeqCst);
                        let net_speed = net_tracker.update();
                        let disk_speed = disk_tracker.update();
                        let verifying = active_verifications.load(Ordering::SeqCst);
                        let downloading = active_downloads.load(Ordering::SeqCst);
                        let phase = if verifying > 0 || install_current > 0 { 4 } else if downloading > 0 { 2 } else { 0 };
                        progress(download_current, download_total, install_current, install_total, net_speed, disk_speed, phase);
                    }
                }
            });

            let injector = Arc::new(Injector::<KuroResource>::new());
            let mut workers = Vec::new();
            let mut stealers_list = Vec::new();
            for _ in 0..6 { let w = Worker::<KuroResource>::new_fifo(); stealers_list.push(w.stealer()); workers.push(w); }
            let stealers = Arc::new(stealers_list);
            for task in files.resource.into_iter() { injector.push(task); }
            let file_sem = Arc::new(tokio::sync::Semaphore::new(6));

            let mut handles = Vec::with_capacity(6);
            for _i in 0..workers.len() {
                let local_worker = workers.pop().unwrap();
                let stealers = stealers.clone();
                let injector = injector.clone();
                let file_sem = file_sem.clone();
                let download_counter = download_counter.clone();
                let install_counter = install_counter.clone();
                let active_verifications = active_verifications.clone();
                let active_downloads = active_downloads.clone();
                let net_tracker = net_tracker.clone();
                let disk_tracker = disk_tracker.clone();
                let chunk_res = base_resources.clone();
                let chunks_zip = base_zip.clone();
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
                            let chunk_res = chunk_res.clone();
                            let chunks_zip = chunks_zip.clone();
                            let staging = staging.clone();
                            let client = client.clone();
                            let cancel_token = cancel_token.clone();
                            let verified_files = verified_files.clone();
                            let failed_chunks = failed_chunks.clone();
                            async move {
                                if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { drop(permit); return; } }
                                let staging_dir = staging.join(chunk_task.dest.clone());
                                let is_raw = !chunk_task.dest.ends_with(".krzip") && !chunk_task.dest.ends_with(".krdiff") && !chunk_task.dest.ends_with(".krpdiff");

                                let mut already_verified = false;
                                if let Some(vf) = &verified_files {
                                    let v = vf.lock().unwrap();
                                    if v.contains(&chunk_task.dest) { already_verified = true; }
                                }

                                // Pre-credit existing bytes so progress doesn't snap back during resume
                                let existing_size = if staging_dir.exists() { staging_dir.metadata().map(|m| m.len().min(chunk_task.size)).unwrap_or(0) } else { 0 };
                                if existing_size > 0 { download_counter.fetch_add(existing_size, Ordering::SeqCst); }

                                active_verifications.fetch_add(1, Ordering::SeqCst);
                                let cvalid = if already_verified { true } else { validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await };
                                active_verifications.fetch_sub(1, Ordering::SeqCst);

                                if staging_dir.exists() && cvalid {
                                    if !already_verified { if let Some(vf) = &verified_files { vf.lock().unwrap().insert(chunk_task.dest.clone()); } }
                                    let remaining = chunk_task.size.saturating_sub(existing_size);
                                    if remaining > 0 { download_counter.fetch_add(remaining, Ordering::SeqCst); }
                                    // File already staged and validated: credit to install bar
                                    install_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    return;
                                }

                                let pn = chunk_task.dest.clone();
                                let chunk_base = if !is_raw { chunk_res } else { chunks_zip + "/" };
                                let url = format!("{chunk_base}{pn}");
                                let mut last_error = String::new();
                                let mut success = false;
                                let mut cancelled = false;

                                active_downloads.fetch_add(1, Ordering::SeqCst);
                                for attempt in 0..3 {
                                    if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { cancelled = true; break; } }
                                    let dl_result = AsyncDownloader::new(client.clone(), url.clone()).await;
                                    if let Err(e) = dl_result { last_error = e.to_string(); continue; }
                                    // Graceful pause: let an active file finish before pausing.
                                    let mut dl = dl_result.unwrap();
                                    let net_t = net_tracker.clone();
                                    let disk_t = disk_tracker.clone();
                                    // Track only NEW bytes from current file size to avoid overlap with download_counter
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
                                        // File downloaded and validated: credit to install bar
                                        install_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                        success = true;
                                        break;
                                    } else { last_error = format!("Checksum mismatch on attempt {}", attempt + 1); }
                                }
                                active_downloads.fetch_sub(1, Ordering::SeqCst);

                                if !success && !cancelled {
                                    eprintln!("Failed to download preload file {} after 3 retries: {}", pn, last_error);
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
                eprintln!("\n=== Preload completed with {} failed file(s) ===", failures.len());
                for fc in failures.iter() { eprintln!("  - File: {}, Error: {}", fc.file_name, fc.error); }
                eprintln!("Some preload files could not be downloaded. The update may still work.\n");
            }
            drop(failures);
            progress(download_total, download_total, install_total, install_total, 0, 0, 0);
            true
        } else { false }
    }
}
