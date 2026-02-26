use crate::download::game::{Game, Sophon};
use crate::utils::downloader::AsyncDownloader;
use crate::utils::proto::{DeleteFiles, FileChunk, ManifestFile, PatchChunk, PatchFile, SophonDiff, SophonManifest};
use crate::utils::{FailedChunk, SpeedTracker, move_all, validate_checksum};
use crossbeam_deque::{Injector, Steal, Worker};
use prost::Message;
use reqwest_middleware::ClientWithMiddleware;
use std::fs;
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write, copy};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use hdiffpatch_rs::patchers::HDiff;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

impl Sophon for Game {
    async fn download<F>(manifest: String, chunk_base: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let p = Path::new(game_path.as_str()).to_path_buf();
        let dlp = p.join("downloading");
        let dlr = p.join("repairing");
        let dlptch = p.join("patching");

        if dlr.exists() { fs::remove_dir_all(&dlr).unwrap(); }
        if dlptch.exists() { fs::remove_dir_all(&dlptch).unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client(false).await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap().with_cancel_token(cancel_token.clone());
        let file = dl.get_filename().await.to_string();
        let dlm = dl.download(dlp.clone().join(&file), |_, _, _, _| {}).await;

        if dlm.is_ok() {
            let m = fs::File::open(dlp.join(&file).as_path()).unwrap();
            let out = fs::File::create(dlp.join("manifest").as_path()).unwrap();
            let mut decoder = zstd::stream::Decoder::new(BufReader::new(m)).unwrap();
            let mut writer = BufWriter::new(out);
            let rslt = copy(&mut decoder, &mut writer);

            if rslt.is_ok() {
                drop(writer);

                let mut f = fs::OpenOptions::new().read(true).open(dlp.join("manifest").as_path()).unwrap();
                let mut file_contents = Vec::new();
                f.read_to_end(&mut file_contents).unwrap();

                if dlp.join(&file).exists() { fs::remove_file(dlp.join(&file).as_path()).unwrap(); }
                let chunks = dlp.join("chunk");
                let staging = dlp.join("staging");

                if !chunks.exists() { fs::create_dir_all(chunks.clone()).unwrap(); }
                if !staging.exists() { fs::create_dir_all(staging.clone()).unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || { SophonManifest::decode(&mut Cursor::new(&file_contents)).unwrap() }).await.unwrap();

                // Install total = uncompressed file sizes
                let install_total: u64 = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| f.size).sum();
                // Download total = compressed chunk sizes
                let download_total: u64 = decoded.files.iter().filter(|f| f.r#type != 64).flat_map(|f| f.chunks.iter()).map(|c| c.chunk_size).sum();
                // Build a map of file name -> compressed size for tracking skipped files
                let file_compressed_sizes: std::collections::HashMap<String, u64> = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| (f.name.clone(), f.chunks.iter().map(|c| c.chunk_size).sum())).collect();
                let file_compressed_sizes = Arc::new(file_compressed_sizes);
                let progress_counter = Arc::new(AtomicU64::new(0));
                // Track compressed bytes satisfied without downloading in this run.
                // Bytes that are actually downloaded come from net_tracker.
                let download_counter = Arc::new(AtomicU64::new(0));
                // Phase tracking using counters to avoid flickering
                // Count of files currently being verified (checking if they exist/valid)
                let active_verifications = Arc::new(AtomicU64::new(0));
                // Count of files currently being downloaded (actual network downloads)
                let active_downloads = Arc::new(AtomicU64::new(0));
                // Count of files currently being validated (final checksum validation)
                let active_validations = Arc::new(AtomicU64::new(0));
                // Track failed chunks for reporting/retry
                let failed_chunks: Arc<std::sync::Mutex<Vec<FailedChunk>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
                let net_tracker = Arc::new(SpeedTracker::new());
                let disk_tracker = Arc::new(SpeedTracker::new());
                let progress = Arc::new(progress);

                // Monitor task for real-time progress/speed reporting using EMA smoothing
                // download_counter tracks pre-validated files' compressed sizes, net_tracker.get_total() tracks downloaded bytes
                let monitor_handle = tokio::spawn({
                    let progress_counter = progress_counter.clone();
                    let download_counter = download_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let active_validations = active_validations.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let progress = progress.clone();
                    async move {
                        loop {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            let validated_download = download_counter.load(Ordering::SeqCst);
                            let active_download = net_tracker.get_total();
                            let download_current = validated_download.saturating_add(active_download).min(download_total);
                            let install_current = progress_counter.load(Ordering::SeqCst);
                            let net_speed = net_tracker.update();
                            let disk_speed = disk_tracker.update();
                            // Determine phase based on counters - downloading takes priority
                            // Phase: 0=idle, 1=verifying, 2=downloading, 3=installing, 4=validating, 5=moving
                            let verifying = active_verifications.load(Ordering::SeqCst);
                            let downloading = active_downloads.load(Ordering::SeqCst);
                            let validating = active_validations.load(Ordering::SeqCst);
                            let phase = if downloading > 0 {
                                2 // downloading (actual network downloads happening)
                            } else if validating > 0 {
                                4 // validating (final checksum validation)
                            } else if verifying > 0 {
                                1 // verifying (checking existing files)
                            } else {
                                0 // idle
                            };
                            progress(download_current, download_total, install_current, install_total, net_speed, disk_speed, phase);
                        }
                    }
                });

                // Start of download code
                let injector = Arc::new(Injector::<ManifestFile>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..5 {
                    let w = Worker::<ManifestFile>::new_fifo();
                    stealers_list.push(w.stealer());
                    workers.push(w);
                }
                let stealers = Arc::new(stealers_list);
                for task in decoded.files.into_iter() { injector.push(task); }
                let file_sem = Arc::new(tokio::sync::Semaphore::new(5));

                // Spawn worker tasks
                let mut handles = Vec::with_capacity(5);
                for _i in 0..workers.len() {
                    let local_worker = workers.pop().unwrap();
                    let stealers = stealers.clone();
                    let injector = injector.clone();
                    let file_sem = file_sem.clone();

                    let stealers = stealers.clone();
                    let progress_counter = progress_counter.clone();
                    let download_counter = download_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let active_validations = active_validations.clone();
                    let file_compressed_sizes = file_compressed_sizes.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let progress_cb = progress.clone();
                    let chunks_dir = chunks.clone();
                    let staging_dir = staging.clone();
                    let chunk_base = chunk_base.clone();
                    let client = client.clone();
                    let chunk_base = chunk_base.clone();
                    let client = client.clone();
                    let cancel_token = cancel_token.clone();
                    let verified_files = verified_files.clone();
                    let failed_chunks = failed_chunks.clone();

                    let mut retry_tasks = Vec::new();
                    let handle = tokio::task::spawn(async move {
                        loop {
                            if let Some(token) = &cancel_token {
                                if token.load(Ordering::Relaxed) { break; }
                            }
                            let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                                    for s in stealers.iter() {
                                        if let Steal::Success(t) = s.steal() { return Some(t); }
                                    }
                                    None
                                });
                            let Some(chunk_task) = job else { break; };
                            let permit = file_sem.clone().acquire_owned().await.unwrap();

                            let ct = tokio::spawn({
                                let progress_counter = progress_counter.clone();
                                let download_counter = download_counter.clone();
                                let active_verifications = active_verifications.clone();
                                let active_downloads = active_downloads.clone();
                                let active_validations = active_validations.clone();
                                let file_compressed_sizes = file_compressed_sizes.clone();
                                let net_tracker = net_tracker.clone();
                                let disk_tracker = disk_tracker.clone();
                                let progress_cb = progress_cb.clone();
                                let chunks_dir = chunks_dir.clone();
                                let staging_dir = staging_dir.clone();
                                let chunk_base = chunk_base.clone();
                                let client = client.clone();
                                let client = client.clone();
                                let cancel_token = cancel_token.clone();
                                let verified_files = verified_files.clone();
                                let failed_chunks = failed_chunks.clone();
                                async move {
                                    if let Some(token) = &cancel_token {
                                        if token.load(Ordering::Relaxed) {
                                            drop(permit);
                                            return;
                                        }
                                    }
                                    // Track verification phase (checking if file exists/valid)
                                    active_verifications.fetch_add(1, Ordering::SeqCst);
                                    let downloaded_this_run = process_file_chunks(chunk_task.clone(), chunks_dir.clone(), staging_dir.clone(), chunk_base.clone(), client.clone(), false, cancel_token.clone(), net_tracker.clone(), disk_tracker.clone(), verified_files.clone(), Some(active_downloads.clone()), Some(failed_chunks.clone())).await;
                                    active_verifications.fetch_sub(1, Ordering::SeqCst);

                                    // Track validating phase (final checksum validation)
                                    active_validations.fetch_add(1, Ordering::SeqCst);
                                    let fp = staging_dir.join(chunk_task.clone().name);
                                    validate_file(chunk_task.clone(), chunk_base.clone(), chunks_dir.clone(), staging_dir.clone(), fp.clone(), client.clone(), progress_counter.clone(), download_counter.clone(), file_compressed_sizes.clone(), progress_cb.clone(), install_total, false, net_tracker.clone(), disk_tracker.clone(), verified_files.clone(), downloaded_this_run).await;
                                    active_validations.fetch_sub(1, Ordering::SeqCst);
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

                // Check for failed chunks and report them
                let failures = failed_chunks.lock().unwrap();
                if !failures.is_empty() {
                    eprintln!("\n=== Download completed with {} failed chunk(s) ===", failures.len());
                    for fc in failures.iter() { eprintln!("  - File: {}, Chunk: {}, Error: {}", fc.file_name, fc.chunk_name, fc.error); }
                    eprintln!("Please run 'Game Repair' after this download completes to fix affected files.\n");
                }
                drop(failures);

                monitor_handle.abort();
                // All files are complete make sure we report done just in case
                // Phase 5 = moving
                progress(download_total, download_total, install_total, install_total, 0, 0, 5);
                // Move from "staging" to "game_path" and delete "downloading" directory
                let moved = move_all(Path::new(&staging), Path::new(&game_path)).await;
                if moved.is_ok() { fs::remove_dir_all(dlp.as_path()).unwrap(); }
                true
            } else { false }
        } else { false }
    }

    async fn patch<F>(manifest: String, version: String, chunk_base: String, game_path: String, preloaded: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.join("patching");
        let dlp = p.join("downloading");
        let dlr = p.join("repairing");

        if dlp.exists() { fs::remove_dir_all(&dlp).unwrap(); }
        if dlr.exists() { fs::remove_dir_all(&dlr).unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client(false).await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap().with_cancel_token(cancel_token.clone());
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _, _, _| {}).await;

        if dll.is_ok() {
            let m = fs::File::open(p.join(&file).as_path()).unwrap();
            let out = fs::File::create(p.join("manifest").as_path()).unwrap();
            let mut decoder = zstd::stream::Decoder::new(BufReader::new(m)).unwrap();
            let mut writer = BufWriter::new(out);
            let rslt = copy(&mut decoder, &mut writer);

            if rslt.is_ok() {
                writer.flush().unwrap();

                let mut f = fs::File::open(p.join("manifest").as_path()).unwrap();
                let mut file_contents = Vec::new();
                f.read_to_end(&mut file_contents).unwrap();

                if p.join(&file).exists() { fs::remove_file(p.join(&file).as_path()).unwrap(); }
                let chunks = p.join("chunk");
                let staging = p.join("staging");

                if !chunks.exists() && !preloaded { fs::create_dir_all(chunks.clone()).unwrap(); }
                if !staging.exists() && !preloaded { fs::create_dir_all(staging.clone()).unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || { SophonDiff::decode(&mut Cursor::new(&file_contents)).unwrap() }).await.unwrap();

                let install_total: u64 = decoded.files.iter().map(|f| { f.chunks.iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).map(|(_v, _chunk)| f.size).sum::<u64>() }).sum();
                let download_total = install_total;
                let delete_files_list = decoded.delete_files.clone();
                let progress_counter = Arc::new(AtomicU64::new(0));
                let download_counter = Arc::new(AtomicU64::new(0));
                let net_tracker = Arc::new(SpeedTracker::new());
                let disk_tracker = Arc::new(SpeedTracker::new());
                let active_verifications = Arc::new(AtomicU64::new(0));
                let active_downloads = Arc::new(AtomicU64::new(0));
                let active_installs = Arc::new(AtomicU64::new(0));
                let active_validations = Arc::new(AtomicU64::new(0));
                let progress = Arc::new(progress);

                let monitor_handle = tokio::spawn({
                    let progress_counter = progress_counter.clone();
                    let download_counter = download_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let active_installs = active_installs.clone();
                    let active_validations = active_validations.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let progress = progress.clone();
                    async move {
                        loop {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            let install_current = progress_counter.load(Ordering::SeqCst);
                            let disk_speed = disk_tracker.update();
                            let verifying = active_verifications.load(Ordering::SeqCst);
                            let downloading = active_downloads.load(Ordering::SeqCst);
                            let installing = active_installs.load(Ordering::SeqCst);
                            let validating = active_validations.load(Ordering::SeqCst);
                            let phase = if downloading > 0 { 2 } else if installing > 0 { 3 } else if validating > 0 { 4 } else if verifying > 0 { 1 } else { 0 };
                            if preloaded {
                                progress(0, 0, install_current, install_total, 0, disk_speed, phase);
                            } else {
                                let validated_download = download_counter.load(Ordering::SeqCst);
                                let download_current = validated_download.saturating_add(net_tracker.get_total()).min(download_total);
                                let net_speed = net_tracker.update();
                                progress(download_current, download_total, install_current, install_total, net_speed, disk_speed, phase);
                            }
                        }
                    }
                });

                let injector = Arc::new(Injector::<PatchFile>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..5 {
                    let w = Worker::<PatchFile>::new_fifo();
                    stealers_list.push(w.stealer());
                    workers.push(w);
                }
                let stealers = Arc::new(stealers_list);
                for task in decoded.files.into_iter() { injector.push(task); }
                let file_sem = Arc::new(tokio::sync::Semaphore::new(5));

                // Spawn worker tasks
                let mut handles = Vec::with_capacity(5);
                for _i in 0..workers.len() {
                    let local_worker = workers.pop().unwrap();
                    let stealers = stealers.clone();
                    let injector = injector.clone();
                    let file_sem = file_sem.clone();

                    let progress_counter = progress_counter.clone();
                    let download_counter = download_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let active_installs = active_installs.clone();
                    let active_validations = active_validations.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let chunks_dir = chunks.clone();
                    let staging_dir = staging.clone();
                    let chunk_base = chunk_base.clone();
                    let version = version.clone();
                    let client = client.clone();
                    let cancel_token = cancel_token.clone();
                    let mainp = mainp.clone();
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
                                let progress_counter = progress_counter.clone();
                                let active_verifications = active_verifications.clone();
                                let active_downloads = active_downloads.clone();
                                let active_installs = active_installs.clone();
                                let active_validations = active_validations.clone();
                                let net_tracker = net_tracker.clone();
                                let disk_tracker = disk_tracker.clone();
                                let chunks_dir = chunks_dir.clone();
                                let staging_dir = staging_dir.clone();
                                let chunk_base = chunk_base.clone();
                                let version = version.clone();
                                let client = client.clone();
                                let cancel_token = cancel_token.clone();
                                let mainp = mainp.clone();
                                let verified_files = verified_files.clone();
                                let download_counter = download_counter.clone();
                                async move {
                                    if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { drop(permit); return; } }

                                    let already_verified = if let Some(vf) = &verified_files {
                                        let v = vf.lock().unwrap();
                                        v.contains(&chunk_task.name)
                                    } else { false };
                                    if already_verified {
                                        progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                        if !preloaded { download_counter.fetch_add(chunk_task.size, Ordering::SeqCst); }
                                        drop(permit);
                                        return;
                                    }

                                    let ff = Arc::new(chunk_task.clone());
                                    let output_path = staging_dir.join(chunk_task.name.clone());

                                    active_verifications.fetch_add(1, Ordering::SeqCst);
                                    let valid = validate_checksum(output_path.as_path(), chunk_task.clone().md5.to_ascii_lowercase()).await;
                                    active_verifications.fetch_sub(1, Ordering::SeqCst);

                                    if output_path.exists() && valid {
                                        progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                        if !preloaded { download_counter.fetch_add(chunk_task.size, Ordering::SeqCst); }
                                        if let Some(vf) = &verified_files {
                                            let mut v = vf.lock().unwrap();
                                            v.insert(chunk_task.name.clone());
                                        }
                                        drop(permit);
                                        return;
                                    } else {
                                        if let Some(parent) = output_path.parent() { fs::create_dir_all(parent).unwrap(); }
                                    }

                                    let filtered: Vec<(String, PatchChunk)> = chunk_task.chunks.clone().into_iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).collect();

                                    // File has patches to apply
                                    if !filtered.is_empty() {
                                        active_installs.fetch_add(1, Ordering::SeqCst);
                                        for (_v, chunk) in filtered.into_iter() {
                                            let output_path = output_path.clone();

                                            let pn = chunk.patch_name;
                                            let chunkp = chunks_dir.join(pn.clone());
                                            let diffp = chunks_dir.join(format!("{}.hdiff", chunk.patch_md5));

                                            // User has predownloaded validate each chunk and apply patches
                                            if preloaded {
                                                let r = validate_checksum(chunkp.as_path(), chunk.patch_md5.to_ascii_lowercase()).await;
                                                if r {
                                                    if chunk.original_filename.is_empty() {
                                                        // Chunk is not a hdiff patchable, copy it over
                                                        let mut chunk_file = fs::File::open(chunkp.as_path()).unwrap();
                                                        chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).unwrap();
                                                        let mut r = vec![0u8; chunk.patch_length as usize];
                                                        chunk_file.read_exact(&mut r).unwrap();
                                                        let is_hdiff = r.starts_with(b"HDIFF13");

                                                        // nap edge case ffs
                                                        if is_hdiff {
                                                            let mut output = fs::File::create(&diffp).unwrap();
                                                            let mut cursor = Cursor::new(&r);
                                                            copy(&mut cursor, &mut output).unwrap();
                                                            output.flush().unwrap();
                                                            drop(output);

                                                            let of = mainp.join(&ff.name.clone());
                                                            let is_dir = ff.name.ends_with("/");
                                                            if !of.exists() {
                                                                if is_dir {
                                                                    fs::create_dir_all(&of).unwrap();
                                                                } else {
                                                                    if let Some(parent) = of.parent() { fs::create_dir_all(parent).unwrap(); }
                                                                    fs::File::create(&of).unwrap();
                                                                }
                                                            } else {
                                                                let _r = fs::remove_file(&of);
                                                                match _r {
                                                                    Ok(_) => { fs::File::create(&of).unwrap(); }
                                                                    Err(_) => {}
                                                                }
                                                            }
                                                            let mut hdiff = HDiff::new(of.to_str().unwrap().to_string(), diffp.to_str().unwrap().to_string().to_string(), output_path.to_str().unwrap().to_string());
                                                            let status = hdiff.apply();
                                                            if !status { eprintln!("Failed to hpatchz without original_filename (has preload)"); }
                                                        } else {
                                                            let mut output = fs::File::create(&output_path).unwrap();
                                                            let mut cursor = Cursor::new(&r);
                                                            copy(&mut cursor, &mut output).unwrap();
                                                            output.flush().unwrap();
                                                            drop(output);
                                                        }
                                                    } else {
                                                        // Chunk is hdiff patchable, patch it
                                                        let mut output = fs::File::create(&diffp).unwrap();
                                                        let mut chunk_file = fs::File::open(chunkp.as_path()).unwrap();

                                                        chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).unwrap();
                                                        let mut r = chunk_file.take(chunk.patch_length);
                                                        copy(&mut r, &mut output).unwrap();
                                                        output.flush().unwrap();
                                                        drop(output);

                                                        let of = mainp.join(&chunk.original_filename);
                                                        let mut hdiff = HDiff::new(of.to_str().unwrap().to_string(), diffp.to_str().unwrap().to_string().to_string(), output_path.to_str().unwrap().to_string());
                                                        if of.exists() {
                                                            let status = hdiff.apply();
                                                            if !status { eprintln!("Failed to hpatchz"); } }
                                                    }
                                                } else { continue; }
                                            } else {
                                                active_downloads.fetch_add(1, Ordering::SeqCst);
                                                let mut dl = AsyncDownloader::new(client.clone(), format!("{chunk_base}/{pn}").to_string()).await.unwrap().with_cancel_token(cancel_token.clone());

                                                let net_t = net_tracker.clone();
                                                let disk_t = disk_tracker.clone();

                                                let dlf = dl.download(chunkp.clone(), move |_, _, ns, ds| {
                                                    net_t.add_bytes(ns);
                                                    disk_t.add_bytes(ds);
                                                }).await;
                                                active_downloads.fetch_sub(1, Ordering::SeqCst);

                                                if dlf.is_ok() && chunkp.exists() {
                                                    let r = validate_checksum(chunkp.as_path(), chunk.patch_md5.to_ascii_lowercase()).await;
                                                    if r {
                                                        if chunk.original_filename.is_empty() {
                                                            // Chunk is not a hdiff patchable, copy it over
                                                            let mut chunk_file = match fs::File::open(chunkp.as_path()) { Ok(f) => f, Err(e) => { eprintln!("Failed to open chunk file {}: {}", chunkp.display(), e); continue; } };
                                                            if let Err(e) = chunk_file.seek(SeekFrom::Start(chunk.patch_offset)) { eprintln!("Failed to seek chunk file: {}", e); continue; }
                                                            let mut r = vec![0u8; chunk.patch_length as usize];
                                                            if let Err(e) = chunk_file.read_exact(&mut r) { eprintln!("Failed to read chunk file: {}", e); continue; }
                                                            let is_hdiff = r.starts_with(b"HDIFF13");

                                                            // nap edge case ffs
                                                            if is_hdiff {
                                                                let mut output = fs::File::create(&diffp).unwrap();
                                                                let mut cursor = Cursor::new(&r);
                                                                copy(&mut cursor, &mut output).unwrap();
                                                                output.flush().unwrap();
                                                                drop(output);

                                                                let of = mainp.join(&ff.name.clone());
                                                                let is_dir = ff.name.ends_with("/");
                                                                if !of.exists() {
                                                                    if is_dir {
                                                                        fs::create_dir_all(&of).unwrap();
                                                                    } else {
                                                                        if let Some(parent) = of.parent() { fs::create_dir_all(parent).unwrap(); }
                                                                        fs::File::create(&of).unwrap();
                                                                    }
                                                                } else {
                                                                    let _r = fs::remove_file(&of);
                                                                    match _r {
                                                                        Ok(_) => { fs::File::create(&of).unwrap(); }
                                                                        Err(_) => {}
                                                                    }
                                                                }
                                                                let mut hdiff = HDiff::new(of.to_str().unwrap().to_string(), diffp.to_str().unwrap().to_string().to_string(), output_path.to_str().unwrap().to_string());
                                                                let status = hdiff.apply();
                                                                if !status { eprintln!("Failed to hpatchz without original_filename (no preload)"); }
                                                            } else {
                                                                let mut output = fs::File::create(&output_path).unwrap();
                                                                let mut cursor = Cursor::new(&r);
                                                                copy(&mut cursor, &mut output).unwrap();
                                                                output.flush().unwrap();
                                                                drop(output);
                                                            }
                                                        } else {
                                                            // Chunk is hdiff patchable, patch it
                                                            let mut output = match fs::File::create(&diffp) { Ok(f) => f, Err(e) => { eprintln!("Failed to create diff file {}: {}", diffp.display(), e); continue; } };
                                                            let mut chunk_file = match fs::File::open(chunkp.as_path()) { Ok(f) => f, Err(e) => { eprintln!("Failed to open chunk file {}: {}", chunkp.display(), e); continue; } };
                                                            if let Err(e) = chunk_file.seek(SeekFrom::Start(chunk.patch_offset)) { eprintln!("Failed to seek chunk file: {}", e); continue; }
                                                            let mut r = chunk_file.take(chunk.patch_length);
                                                            if let Err(e) = copy(&mut r, &mut output) { eprintln!("Failed to copy chunk data: {}", e); continue; }
                                                            let _ = output.flush();
                                                            drop(output);

                                                            let of = mainp.join(&chunk.original_filename);
                                                            if of.exists() {
                                                                let mut hdiff = HDiff::new(of.to_str().unwrap().to_string(), diffp.to_str().unwrap().to_string().to_string(), output_path.to_str().unwrap().to_string());
                                                                let status = hdiff.apply();
                                                                if !status { eprintln!("Failed to hpatchz!"); } }
                                                        }
                                                    } else { continue; }
                                                }
                                            } // preload check end
                                        }
                                        active_installs.fetch_sub(1, Ordering::SeqCst);
                                        active_validations.fetch_add(1, Ordering::SeqCst);
                                        let r2 = validate_checksum(output_path.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
                                        active_validations.fetch_sub(1, Ordering::SeqCst);
                                        if r2 {
                                            progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                            if let Some(vf) = &verified_files {
                                                let mut v = vf.lock().unwrap();
                                                v.insert(chunk_task.name.clone());
                                            }
                                        }
                                    }
                                    drop(permit);
                                }
                            }); // end task
                            retry_tasks.push(ct);
                        }
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
                if preloaded {
                    progress(0, 0, install_total, install_total, 0, 0, 0);
                } else {
                    progress(download_total, download_total, install_total, install_total, 0, 0, 0);
                }
                let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                if moved.is_ok() {
                    if preloaded {} else { fs::remove_dir_all(p.as_path()).unwrap(); }
                    let purge_list: Vec<(String, DeleteFiles)> = delete_files_list.into_iter().filter(|(v, _f)| version.as_str() == v.as_str()).collect();
                    if !purge_list.is_empty() {
                        for (_v, df) in purge_list.into_iter() {
                            for f in df.files {
                                let fp = mainp.join(&f.name);
                                if fp.exists() { fs::remove_file(&fp).unwrap(); };
                            }
                        }
                    }
                }
                true
            } else { false }
        } else { false }
    }

    async fn repair_game<F>(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str());
        let mainpbuf = mainp.to_path_buf();
        let p = mainpbuf.join("repairing");
        let dlp = p.join("downloading");
        let dlptch = p.join("patching");

        if dlp.exists() { fs::remove_dir_all(&dlp).unwrap(); }
        if dlptch.exists() { fs::remove_dir_all(&dlptch).unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client(false).await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap().with_cancel_token(cancel_token.clone());
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _, _, _| {}).await;

        if dll.is_ok() {
            let m = fs::File::open(p.join(&file).as_path()).unwrap();
            let out = fs::File::create(p.join("manifest").as_path()).unwrap();
            let mut decoder = zstd::stream::Decoder::new(BufReader::new(m)).unwrap();
            let mut writer = BufWriter::new(out);
            let rslt = copy(&mut decoder, &mut writer);

            if rslt.is_ok() {
                writer.flush().unwrap();
                let mut f = fs::File::open(p.join("manifest").as_path()).unwrap();
                let mut file_contents = Vec::new();
                f.read_to_end(&mut file_contents).unwrap();

                if p.join(&file).exists() { fs::remove_file(p.join(&file).as_path()).unwrap(); }
                let chunks = p.join("chunk");

                if !chunks.exists() { fs::create_dir_all(chunks.clone()).unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || { SophonManifest::decode(&mut Cursor::new(&file_contents)).unwrap() }).await.unwrap();

                // Install total = uncompressed file sizes
                let install_total: u64 = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| f.size).sum();
                // Download total = compressed chunk sizes
                let download_total: u64 = decoded.files.iter().filter(|f| f.r#type != 64).flat_map(|f| f.chunks.iter()).map(|c| c.chunk_size).sum();
                // Build a map of file name -> compressed size for tracking skipped files
                let file_compressed_sizes: std::collections::HashMap<String, u64> = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| (f.name.clone(), f.chunks.iter().map(|c| c.chunk_size).sum())).collect();
                let file_compressed_sizes = Arc::new(file_compressed_sizes);
                let progress_counter = Arc::new(AtomicU64::new(0));
                // Track compressed bytes satisfied without downloading in this run.
                // Bytes that are actually downloaded come from net_tracker.
                let download_counter = Arc::new(AtomicU64::new(0));
                // Phase tracking using counters to avoid flickering
                let active_verifications = Arc::new(AtomicU64::new(0));
                let active_downloads = Arc::new(AtomicU64::new(0));
                let active_validations = Arc::new(AtomicU64::new(0));
                // Track failed chunks for reporting/retry
                let failed_chunks: Arc<std::sync::Mutex<Vec<FailedChunk>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
                let net_tracker = Arc::new(SpeedTracker::new());
                let disk_tracker = Arc::new(SpeedTracker::new());
                let progress = Arc::new(progress);

                // Monitor task for real-time progress/speed reporting using EMA smoothing
                // download_counter tracks pre-validated files' compressed sizes, net_tracker.get_total() tracks downloaded bytes
                let monitor_handle = tokio::spawn({
                    let progress_counter = progress_counter.clone();
                    let download_counter = download_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let active_validations = active_validations.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let progress = progress.clone();
                    async move {
                        loop {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            let validated_download = download_counter.load(Ordering::SeqCst);
                            let active_download = net_tracker.get_total();
                            let download_current = validated_download.saturating_add(active_download).min(download_total);
                            let install_current = progress_counter.load(Ordering::SeqCst);
                            let net_speed = net_tracker.update();
                            let disk_speed = disk_tracker.update();
                            // Determine phase based on counters - downloading takes priority
                            // Phase: 0=idle, 1=verifying, 2=downloading, 3=installing, 4=validating, 5=moving
                            let verifying = active_verifications.load(Ordering::SeqCst);
                            let downloading = active_downloads.load(Ordering::SeqCst);
                            let validating = active_validations.load(Ordering::SeqCst);
                            let phase = if downloading > 0 {
                                2 // downloading (actual network downloads happening)
                            } else if validating > 0 {
                                4 // validating (final checksum validation)
                            } else if verifying > 0 {
                                1 // verifying (checking existing files)
                            } else {
                                0 // idle
                            };
                            progress(download_current, download_total, install_current, install_total, net_speed, disk_speed, phase);
                        }
                    }
                });

                // Start of download code
                let injector = Arc::new(Injector::<ManifestFile>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..5 {
                    let w = Worker::<ManifestFile>::new_fifo();
                    stealers_list.push(w.stealer());
                    workers.push(w);
                }
                let stealers = Arc::new(stealers_list);
                for task in decoded.files.into_iter() { injector.push(task); }
                let file_sem = Arc::new(tokio::sync::Semaphore::new(5));

                // Spawn worker tasks
                let mut handles = Vec::with_capacity(5);
                for _i in 0..workers.len() {
                    let local_worker = workers.pop().unwrap();
                    let stealers = stealers.clone();
                    let injector = injector.clone();
                    let file_sem = file_sem.clone();

                    let stealers = stealers.clone();
                    let progress_counter = progress_counter.clone();
                    let download_counter = download_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let active_validations = active_validations.clone();
                    let file_compressed_sizes = file_compressed_sizes.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let progress_cb = progress.clone();
                    let chunks_dir = chunks.clone();
                    let mainp = mainpbuf.clone();
                    let chunk_base = chunk_base.clone();
                    let client = client.clone();
                    let cancel_token = cancel_token.clone();
                    let verified_files = verified_files.clone();
                    let failed_chunks = failed_chunks.clone();

                    let mut retry_tasks = Vec::new();
                    let handle = tokio::task::spawn(async move {
                        loop {
                            if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { break; } }
                            let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                                    for s in stealers.iter() {
                                        if let Steal::Success(t) = s.steal() { return Some(t); }
                                    }
                                    None
                                });
                            let Some(chunk_task) = job else { break; };
                            let permit = file_sem.clone().acquire_owned().await.unwrap();

                            let ct = tokio::task::spawn({
                                let progress_counter = progress_counter.clone();
                                let download_counter = download_counter.clone();
                                let active_verifications = active_verifications.clone();
                                let active_downloads = active_downloads.clone();
                                let active_validations = active_validations.clone();
                                let file_compressed_sizes = file_compressed_sizes.clone();
                                let net_tracker = net_tracker.clone();
                                let disk_tracker = disk_tracker.clone();
                                let progress_cb = progress_cb.clone();
                                let mainp = mainp.clone();
                                let chunks_dir = chunks_dir.clone();
                                let chunk_base = chunk_base.clone();
                                let client = client.clone();
                                let cancel_token = cancel_token.clone();
                                let verified_files = verified_files.clone();
                                let failed_chunks = failed_chunks.clone();
                                async move {
                                    if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { drop(permit); return; } }
                                    // Track verification phase (checking if file exists/valid)
                                    active_verifications.fetch_add(1, Ordering::SeqCst);
                                    let downloaded_this_run = process_file_chunks(chunk_task.clone(), chunks_dir.clone(), mainp.clone(), chunk_base.clone(), client.clone(), is_fast, cancel_token.clone(), net_tracker.clone(), disk_tracker.clone(), verified_files.clone(), Some(active_downloads.clone()), Some(failed_chunks.clone())).await;
                                    active_verifications.fetch_sub(1, Ordering::SeqCst);

                                    // Track validating phase (final checksum validation)
                                    active_validations.fetch_add(1, Ordering::SeqCst);
                                    let fp = mainp.join(chunk_task.clone().name);
                                    validate_file(chunk_task.clone(), chunk_base.clone(), chunks_dir.clone(), mainp.clone(), fp.clone(), client.clone(), progress_counter.clone(), download_counter.clone(), file_compressed_sizes.clone(), progress_cb.clone(), install_total, is_fast, net_tracker.clone(), disk_tracker.clone(), verified_files.clone(), downloaded_this_run).await;
                                    active_validations.fetch_sub(1, Ordering::SeqCst);
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

                // Check for failed chunks and report them
                let failures = failed_chunks.lock().unwrap();
                if !failures.is_empty() {
                    eprintln!("\n=== Repair completed with {} failed chunk(s) ===", failures.len());
                    for fc in failures.iter() { eprintln!("  - File: {}, Chunk: {}, Error: {}", fc.file_name, fc.chunk_name, fc.error); }
                    eprintln!("Please re-run 'Game Repair' to fix remaining affected files.\n");
                }
                drop(failures);

                monitor_handle.abort();
                // All files are complete make sure we report done just in case
                progress(download_total, download_total, install_total, install_total, 0, 0, 0);
                if p.exists() { fs::remove_dir_all(p.as_path()).unwrap(); }
                true
            } else { false }
        } else { false }
    }

    async fn preload<F>(manifest: String, version: String, chunk_base: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.join("patching");
        let dlr = mainp.join("repairing");
        let dlp = mainp.join("downloading");

        // If these directories exist delete them for safety
        if dlr.exists() { fs::remove_dir_all(&dlr).unwrap(); }
        if dlp.exists() { fs::remove_dir_all(&dlp).unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client(false).await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap().with_cancel_token(cancel_token.clone());
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _, _, _| {}).await;

        if dll.is_ok() {
            let m = fs::File::open(p.join(&file).as_path()).unwrap();
            let out = fs::File::create(p.join("manifest").as_path()).unwrap();
            let mut decoder = zstd::stream::Decoder::new(BufReader::new(m)).unwrap();
            let mut writer = BufWriter::new(out);
            let rslt = std::io::copy(&mut decoder, &mut writer);

            if rslt.is_ok() {
                writer.flush().unwrap();

                let mut f = fs::File::open(p.join("manifest").as_path()).unwrap();
                let mut file_contents = Vec::new();
                f.read_to_end(&mut file_contents).unwrap();

                if p.join(&file).exists() { fs::remove_file(p.join(&file).as_path()).unwrap(); }
                if !p.join(".preload").exists() { fs::File::create(p.join(".preload")).unwrap(); }
                let chunks = p.join("chunk");

                if !chunks.exists() { fs::create_dir_all(chunks.clone()).unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || { SophonDiff::decode(&mut Cursor::new(&file_contents)).unwrap() }).await.unwrap();

                let mut seen_total: std::collections::HashSet<String> = std::collections::HashSet::new();
                let download_total: u64 = decoded.files.iter().filter(|f| f.chunks.contains_key(&version)).flat_map(|f| f.chunks.iter().filter(|(v, _)| v.as_str() == version.as_str())).filter(|(_, c)| seen_total.insert(c.patch_name.clone())).map(|(_, c)| c.patch_size).sum();
                let download_counter = Arc::new(AtomicU64::new(0));
                let seen_patches: Arc<std::sync::Mutex<std::collections::HashSet<String>>> = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
                let active_verifications = Arc::new(AtomicU64::new(0));
                let active_downloads = Arc::new(AtomicU64::new(0));
                let active_validations = Arc::new(AtomicU64::new(0));
                let failed_chunks: Arc<std::sync::Mutex<Vec<FailedChunk>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
                let net_tracker = Arc::new(SpeedTracker::new());
                let disk_tracker = Arc::new(SpeedTracker::new());
                let progress = Arc::new(progress);
                progress(download_counter.load(Ordering::SeqCst).min(download_total), download_total, 0, 0, 0, 0, 1);

                let monitor_handle = tokio::spawn({
                    let download_counter = download_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let active_validations = active_validations.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let progress = progress.clone();
                    async move {
                        loop {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            let download_current = download_counter.load(Ordering::SeqCst).min(download_total);
                            let net_speed = net_tracker.update();
                            let disk_speed = disk_tracker.update();
                            let verifying = active_verifications.load(Ordering::SeqCst);
                            let downloading = active_downloads.load(Ordering::SeqCst);
                            let validating = active_validations.load(Ordering::SeqCst);
                            let phase = if downloading > 0 { 2 } else if validating > 0 { 4 } else if verifying > 0 { 1 } else { 0 };
                            progress(download_current, download_total, 0, 0, net_speed, disk_speed, phase);
                        }
                    }
                });

                // Start of download code
                let injector = Arc::new(Injector::<PatchFile>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..5 {
                    let w = Worker::<PatchFile>::new_fifo();
                    stealers_list.push(w.stealer());
                    workers.push(w);
                }
                let stealers = Arc::new(stealers_list);
                for task in decoded.files.into_iter() { injector.push(task); }
                let file_sem = Arc::new(tokio::sync::Semaphore::new(5));

                // Spawn worker tasks
                let mut handles = Vec::with_capacity(5);
                for _i in 0..workers.len() {
                    let local_worker = workers.pop().unwrap();
                    let stealers = stealers.clone();
                    let injector = injector.clone();
                    let file_sem = file_sem.clone();

                    let stealers = stealers.clone();
                    let download_counter = download_counter.clone();
                    let active_verifications = active_verifications.clone();
                    let active_downloads = active_downloads.clone();
                    let active_validations = active_validations.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let chunks_dir = chunks.clone();
                    let chunk_base = chunk_base.clone();
                    let version = version.clone();
                    let client = client.clone();
                    let cancel_token = cancel_token.clone();
                    let verified_files = verified_files.clone();
                    let failed_chunks = failed_chunks.clone();
                    let seen_patches = seen_patches.clone();

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
                                let active_verifications = active_verifications.clone();
                                let active_downloads = active_downloads.clone();
                                let active_validations = active_validations.clone();
                                let net_tracker = net_tracker.clone();
                                let disk_tracker = disk_tracker.clone();
                                let chunks_dir = chunks_dir.clone();
                                let chunk_base = chunk_base.clone();
                                let version = version.clone();
                                let client = client.clone();
                                let cancel_token = cancel_token.clone();
                                let verified_files = verified_files.clone();
                                let failed_chunks = failed_chunks.clone();
                                let seen_patches = seen_patches.clone();
                                async move {
                                    if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { drop(permit); return; } }
                                    active_verifications.fetch_add(1, Ordering::SeqCst);
                                    let already_verified = if let Some(vf) = &verified_files {
                                        let v = vf.lock().unwrap();
                                        v.contains(&chunk_task.name)
                                    } else { false };
                                    let task_name = chunk_task.name.clone();
                                    let filtered: Vec<(String, PatchChunk)> = chunk_task.chunks.into_iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).collect();
                                    active_verifications.fetch_sub(1, Ordering::SeqCst);
                                    if filtered.is_empty() { drop(permit); return; }

                                    if already_verified {
                                        for (_v, chunk) in &filtered {
                                            if !seen_patches.lock().unwrap().insert(chunk.patch_name.clone()) { continue; }
                                            download_counter.fetch_add(chunk.patch_size, Ordering::SeqCst);
                                        }
                                        drop(permit);
                                        return;
                                    }

                                    let mut all_ok = true;
                                    for (_v, chunk) in filtered.into_iter() {
                                        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { drop(permit); return; } }
                                        let pn = chunk.patch_name.clone();
                                        if !seen_patches.lock().unwrap().insert(pn.clone()) { continue; }
                                        let chunkp = chunks_dir.join(&pn);

                                        active_verifications.fetch_add(1, Ordering::SeqCst);
                                        let cvalid = validate_checksum(chunkp.as_path(), chunk.patch_md5.to_ascii_lowercase()).await;
                                        active_verifications.fetch_sub(1, Ordering::SeqCst);

                                        if chunkp.exists() && cvalid {
                                            download_counter.fetch_add(chunk.patch_size, Ordering::SeqCst);
                                            continue;
                                        }

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
                                            let download_counter_cb = download_counter.clone();
                                            let cur_size = chunkp.metadata().map(|m| m.len()).unwrap_or(0);
                                            download_counter.fetch_add(cur_size, Ordering::SeqCst);
                                            let mut last_written = cur_size;
                                            let dlf = dl.download(chunkp.clone(), move |current, _, _, _| {
                                                let diff = current.saturating_sub(last_written);
                                                if diff > 0 { net_t.add_bytes(diff); disk_t.add_bytes(diff); download_counter_cb.fetch_add(diff, Ordering::SeqCst); }
                                                last_written = current;
                                            }).await;
                                            if let Err(e) = &dlf { download_counter.fetch_sub(cur_size, Ordering::SeqCst); last_error = e.to_string(); continue; }

                                            active_validations.fetch_add(1, Ordering::SeqCst);
                                            let cvalid = validate_checksum(chunkp.as_path(), chunk.patch_md5.to_ascii_lowercase()).await;
                                            active_validations.fetch_sub(1, Ordering::SeqCst);
                                            if cvalid { success = true; break; } else {
                                                let written = chunkp.metadata().map(|m| m.len()).unwrap_or(0);
                                                download_counter.fetch_sub(written.min(download_counter.load(Ordering::SeqCst)), Ordering::SeqCst);
                                                last_error = "Checksum mismatch".to_string();
                                                if chunkp.exists() { let _ = tokio::fs::remove_file(&chunkp).await; }
                                            }
                                        }
                                        active_downloads.fetch_sub(1, Ordering::SeqCst);

                                        if cancelled { drop(permit); return; }
                                        if !success {
                                            eprintln!("Failed to download preload chunk {}/{} after 3 retries: {}", chunk_base, pn, last_error);
                                            failed_chunks.lock().unwrap().push(FailedChunk { file_name: task_name.clone(), chunk_name: pn.clone(), error: last_error });
                                            all_ok = false;
                                        }
                                    }

                                    if all_ok {
                                        if let Some(vf) = &verified_files {
                                            let mut v = vf.lock().unwrap();
                                            v.insert(task_name);
                                        }
                                    }
                                    drop(permit);
                                }
                            }); // end task
                            retry_tasks.push(ct);
                        }
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
                let failures = failed_chunks.lock().unwrap();
                if !failures.is_empty() {
                    eprintln!("\n=== Preload completed with {} failed chunk(s) ===", failures.len());
                    for fc in failures.iter() { eprintln!("  - File: {}, Chunk: {}, Error: {}", fc.file_name, fc.chunk_name, fc.error); }
                    eprintln!("Please re-run preload to retry failed chunks.\n");
                }
                drop(failures);
                monitor_handle.abort();
                progress(download_total, download_total, 0, 0, 0, 0, 0);
                true
            } else { false }
        } else { false }
    }
}

async fn process_file_chunks(chunk_task: ManifestFile, chunks_dir: PathBuf, staging_dir: PathBuf, chunk_base: String, client: Arc<ClientWithMiddleware>, is_fast: bool, cancel_token: Option<Arc<AtomicBool>>, net_tracker: Arc<SpeedTracker>, disk_tracker: Arc<SpeedTracker>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>, active_downloads: Option<Arc<AtomicU64>>, failed_chunks: Option<Arc<std::sync::Mutex<Vec<FailedChunk>>>>) -> bool {
    if chunk_task.r#type == 64 { return false; }

    if let Some(vf) = &verified_files {
        let v = vf.lock().unwrap();
        if v.contains(&chunk_task.name) { return false; }
    }

    let fp = staging_dir.join(&chunk_task.name);
    let validstg = if is_fast { fp.metadata().map(|m| m.len() == chunk_task.size).unwrap_or(false) } else { validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await };
    if fp.exists() && validstg {
        return false;
    } else {
        if let Some(parent) = fp.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
    }

    // File needs downloading - track this in active_downloads counter
    if let Some(ref counter) = active_downloads { counter.fetch_add(1, Ordering::SeqCst); }

    let file = tokio::fs::OpenOptions::new().create(true).write(true).open(&fp).await.unwrap();
    file.set_len(chunk_task.size).await.unwrap();
    let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<(Vec<u8>, u64)>(600);

    let writer_handle = tokio::spawn({
        let disk_tracker = disk_tracker.clone();
        async move {
            let mut writer = tokio::io::BufWriter::with_capacity(10240, file);
            while let Some((buf, offset)) = write_rx.recv().await {
                let len = buf.len() as u64;
                writer.seek(SeekFrom::Start(offset)).await.unwrap();
                writer.write_all(&buf).await.unwrap();
                disk_tracker.add_bytes(len);
            }
            writer.flush().await.unwrap();
            drop(writer);
        }
    });

    let injector = Arc::new(Injector::<FileChunk>::new());
    let mut workers = Vec::new();
    let mut stealers_list = Vec::new();
    for _ in 0..12 { let w = Worker::<FileChunk>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
    let stealers = Arc::new(stealers_list);
    for task in chunk_task.chunks.into_iter() { injector.push(task); }

    // Spawn worker tasks
    let file_name = chunk_task.name.clone();
    let mut handles = Vec::with_capacity(12);
    for _i in 0..workers.len() {
        let local_worker = workers.pop().unwrap();
        let stealers = stealers.clone();
        let injector = injector.clone();
        let write_tx = write_tx.clone();

        let chunks_dir = chunks_dir.clone();
        let stealers = stealers.clone();
        let client = Arc::clone(&client);
        let chunk_base = chunk_base.clone();
        let cancel_token = cancel_token.clone();
        let net_tracker = net_tracker.clone();
        let disk_tracker = disk_tracker.clone();
        let failed_chunks = failed_chunks.clone();
        let file_name = file_name.clone();

        let mut retry_tasks = Vec::new();
        let handle = tokio::task::spawn(async move {
            loop {
                if let Some(token) = &cancel_token {
                    if token.load(Ordering::Relaxed) { break; }
                }
                let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                        for s in stealers.iter() {
                            if let Steal::Success(t) = s.steal() { return Some(t); }
                        }
                        None
                    });
                let Some(c) = job else { break; };

                let ct = tokio::spawn({
                    let chunk_path = chunks_dir.join(&c.chunk_name);
                    let chunk_base = chunk_base.clone();
                    let client = client.clone();
                    let write_tx = write_tx.clone();
                    let cancel_token = cancel_token.clone();
                    let net_tracker = net_tracker.clone();
                    let disk_tracker = disk_tracker.clone();
                    let failed_chunks = failed_chunks.clone();
                    let file_name = file_name.clone();
                    async move {
                        if let Some(token) = &cancel_token {
                            if token.load(Ordering::Relaxed) { return; }
                        }
                        let mut dl = match AsyncDownloader::new(client, format!("{}/{}", chunk_base, c.chunk_name)).await {
                            Ok(d) => d.with_cancel_token(cancel_token.clone()),
                            Err(e) => {
                                eprintln!("Failed to init chunk download {}/{}: {}", chunk_base, c.chunk_name, e);
                                if let Some(ref fc) = failed_chunks { fc.lock().unwrap().push(FailedChunk { file_name: file_name.clone(), chunk_name: c.chunk_name.clone(), error: e.to_string() }); }
                                return;
                            }
                        };

                        // Simple byte tracking - just report bytes downloaded
                        let net_t = net_tracker.clone();
                        let disk_t = disk_tracker.clone();
                        let mut last_net = 0u64;
                        let mut last_disk = 0u64;
                        let dl_result = dl.download(chunk_path.clone(), move |current, _total, _net_speed, _disk_speed| {
                                    let net_diff = current.saturating_sub(last_net);
                                    if net_diff > 0 { net_t.add_bytes(net_diff); }
                                    last_net = current;

                                    // Track disk speed from AsyncDownloader as well
                                    // Wait, current in AsyncDownloader refers to bytes WRITTEN to disk?
                                    // Let's check AsyncDownloader::download again.
                                    // Yes, current is written_bytes.
                                    let disk_diff = current.saturating_sub(last_disk);
                                    if disk_diff > 0 { disk_t.add_bytes(disk_diff); }
                                    last_disk = current;
                                },
                            ).await;

                        if dl_result.is_ok() {
                            let valid = validate_checksum(chunk_path.as_path(), c.chunk_md5.to_ascii_lowercase()).await;
                            if valid && chunk_path.exists() {
                                let file = fs::File::open(&chunk_path).unwrap();
                                let mut reader = BufReader::with_capacity(10240, file);
                                let mut decoder = zstd::Decoder::new(&mut reader).unwrap();
                                let mut buf = Vec::with_capacity(c.chunk_decompressed_size as usize);
                                std::io::copy(&mut decoder, &mut buf).unwrap();
                                write_tx.send((buf, c.chunk_on_file_offset)).await.unwrap();
                            }
                        } else {
                            // Check cancel token before retry
                            if let Some(token) = &cancel_token {
                                if token.load(Ordering::Relaxed) { return; }
                            }
                            // Retry the chunk
                            let net_t = net_tracker.clone();
                            let disk_t = disk_tracker.clone();
                            let mut last_net = 0u64;
                            let mut last_disk = 0u64;
                            let dl_result2 = dl.download(chunk_path.clone(), move |current, _total, _net_speed, _disk_speed| {
                                        let net_diff = current.saturating_sub(last_net);
                                        if net_diff > 0 { net_t.add_bytes(net_diff); }
                                        last_net = current;

                                        let disk_diff = current.saturating_sub(last_disk);
                                        if disk_diff > 0 { disk_t.add_bytes(disk_diff); }
                                        last_disk = current;
                                    },
                                ).await;

                            if dl_result2.is_ok() {
                                let valid = validate_checksum(chunk_path.as_path(), c.chunk_md5.to_ascii_lowercase()).await;
                                if valid && chunk_path.exists() {
                                    let file = fs::File::open(&chunk_path).unwrap();
                                    let mut reader = BufReader::with_capacity(10240, file);
                                    let mut decoder = zstd::Decoder::new(&mut reader).unwrap();
                                    let mut buf = Vec::with_capacity(c.chunk_decompressed_size as usize);
                                    std::io::copy(&mut decoder, &mut buf).unwrap();
                                    write_tx.send((buf, c.chunk_on_file_offset)).await.unwrap();
                                }
                            } else {
                                // Check cancel token before second retry
                                if let Some(token) = &cancel_token {
                                    if token.load(Ordering::Relaxed) { return; }
                                }
                                // Retry again
                                let net_t = net_tracker.clone();
                                let disk_t = disk_tracker.clone();
                                let mut last_net = 0u64;
                                let mut last_disk = 0u64;
                                let dl_result3 = dl.download(chunk_path.clone(), move |current, _total, _net_speed, _disk_speed| {
                                            let net_diff = current.saturating_sub(last_net);
                                            if net_diff > 0 { net_t.add_bytes(net_diff); }
                                            last_net = current;

                                            let disk_diff = current.saturating_sub(last_disk);
                                            if disk_diff > 0 { disk_t.add_bytes(disk_diff); }
                                            last_disk = current;
                                        },
                                    ).await;

                                if dl_result3.is_ok() {
                                    let valid = validate_checksum(chunk_path.as_path(), c.chunk_md5.to_ascii_lowercase()).await;
                                    if valid && chunk_path.exists() {
                                        if let Ok(file) = fs::File::open(&chunk_path) {
                                            let mut reader = BufReader::with_capacity(10240, file);
                                            if let Ok(mut decoder) = zstd::Decoder::new(&mut reader) {
                                                let mut buf = Vec::with_capacity(c.chunk_decompressed_size as usize);
                                                if std::io::copy(&mut decoder, &mut buf).is_ok() { let _ = write_tx.send((buf, c.chunk_on_file_offset)).await; }
                                            }
                                        }
                                    }
                                } else {
                                    let err = dl_result3.unwrap_err().to_string();
                                    eprintln!("Download of chunk {}/{} failed 3 times with error {}", chunk_base, c.chunk_name, err);
                                    if let Some(ref fc) = failed_chunks { fc.lock().unwrap().push(FailedChunk { file_name: file_name.clone(), chunk_name: c.chunk_name.clone(), error: err }); }
                                }
                            }
                        }
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
            for t in retry_tasks { let _ = t.await; /* Ignore JoinError - task may have panicked or been cancelled */ }
        });
        handles.push(handle);
    }
    drop(write_tx);
    for handle in handles { let _ = handle.await; /* Ignore JoinError - task may have panicked or been cancelled*/ }
    let _ = writer_handle.await;

    // Done downloading - decrement counter
    if let Some(ref counter) = active_downloads { counter.fetch_sub(1, Ordering::SeqCst); }
    true
}

async fn validate_file<F>(chunk_task: ManifestFile, chunk_base: String, chunks_dir: PathBuf, staging_dir: PathBuf, fp: PathBuf, client: Arc<ClientWithMiddleware>, progress_counter: Arc<AtomicU64>, download_counter: Arc<AtomicU64>, file_compressed_sizes: Arc<std::collections::HashMap<String, u64>>, _progress_cb: Arc<F>, _total_bytes: u64, is_fast: bool, net_tracker: Arc<SpeedTracker>, disk_tracker: Arc<SpeedTracker>, verified_files: Option<Arc<std::sync::Mutex<std::collections::HashSet<String>>>>, downloaded_this_run: bool) where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
    // Get the compressed size for this file
    let compressed_size = file_compressed_sizes.get(&chunk_task.name).copied().unwrap_or(0);
    let mut already_verified = false;
    if let Some(vf) = &verified_files {
        let v = vf.lock().unwrap();
        if v.contains(&chunk_task.name) { already_verified = true; }
    }

    let valid = if already_verified { true } else { validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await };
    if !valid {
        if fp.exists() { if let Err(e) = tokio::fs::remove_file(&fp).await { eprintln!("Failed to delete incomplete file before retry: {}: {}", fp.display(), e); } }
        process_file_chunks(chunk_task.clone(), chunks_dir.clone(), staging_dir.clone(), chunk_base.clone(), client.clone(), is_fast, None, net_tracker.clone(), disk_tracker.clone(), verified_files.clone(), None, None).await;
        let revalid = validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
        if !revalid {
            if fp.exists() { if let Err(e) = tokio::fs::remove_file(&fp).await { eprintln!("Failed to delete incomplete file before re-retry: {}: {}", fp.display(), e); } }
            process_file_chunks(chunk_task.clone(), chunks_dir.clone(), staging_dir.clone(), chunk_base.clone(), client.clone(), is_fast, None, net_tracker.clone(), disk_tracker.clone(), verified_files.clone(), None, None).await;
            let revalid2 = validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
            if !revalid2 {
                if fp.exists() { if let Err(e) = tokio::fs::remove_file(&fp).await { eprintln!("Failed to delete incomplete file before re-re-retry: {}: {}", fp.display(), e); } }
                process_file_chunks(chunk_task.clone(), chunks_dir.clone(), staging_dir.clone(), chunk_base.clone(), client.clone(), is_fast, None, net_tracker.clone(), disk_tracker.clone(), verified_files.clone(), None, None).await;
                let revalid3 = validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
                if !revalid3 { eprintln!("Failed to validate file after 3 retries! Please run game repair after finishing. Affected file: {}", chunk_task.name.clone());
                } else {
                    let _processed = progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                    // download bytes already tracked via net_tracker in process_file_chunks callback
                    // Cleanup chunks - ignore errors (race conditions with other workers, files still in use)
                    for c in &chunk_task.chunks {
                        let chunk_path = chunks_dir.join(&c.chunk_name);
                        let _ = tokio::fs::remove_file(&chunk_path).await;
                    }
                }
            } else {
                let _processed = progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                // download bytes already tracked via net_tracker in process_file_chunks callback
                // Cleanup chunks - ignore errors (race conditions with other workers, files still in use)
                for c in &chunk_task.chunks {
                    let chunk_path = chunks_dir.join(&c.chunk_name);
                    let _ = tokio::fs::remove_file(&chunk_path).await;
                }
            }
        } else {
            let _processed = progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
            // download bytes already tracked via net_tracker in process_file_chunks callback
            // Cleanup chunks - ignore errors (race conditions with other workers, files still in use)
            for c in &chunk_task.chunks {
                let chunk_path = chunks_dir.join(&c.chunk_name);
                let _ = tokio::fs::remove_file(&chunk_path).await;
            }
        }
    } else {
        if valid && !already_verified {
            if let Some(vf) = &verified_files {
                let mut v = vf.lock().unwrap();
                v.insert(chunk_task.name.clone());
            }
        }
        let _processed = progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
        if !downloaded_this_run {
            // Only count files that were already valid locally in this run.
            // Newly downloaded bytes are accounted for by net_tracker.
            download_counter.fetch_add(compressed_size, Ordering::SeqCst);
        }
        // Cleanup chunks - ignore errors (race conditions with other workers, files still in use)
        for c in &chunk_task.chunks {
            let chunk_path = chunks_dir.join(&c.chunk_name);
            let _ = tokio::fs::remove_file(&chunk_path).await;
        }
    }
}
