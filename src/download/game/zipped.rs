use crate::download::game::{Game, Zipped};
use crate::utils::{BeyondPatchIndex, FailedChunk, extract_archive_with_progress, move_all, validate_checksum};
use crate::utils::downloader::AsyncDownloader;
use hdiffpatch_rs::patchers::HDiff;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

impl Zipped for Game {
    async fn download(url: String, hash: String, game_path: String, use_patching_structure: bool, use_repair_structure: bool, progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<Mutex<std::collections::HashSet<String>>>>) -> bool {
        if url.is_empty() || game_path.is_empty() { return false; }

        let p = Path::new(game_path.as_str()).to_path_buf();
        let dlp = p.join("downloading");
        let dlr = p.join("repairing");
        let dlptch = p.join("patching");

        if use_patching_structure {
            if dlp.exists() { std::fs::remove_dir_all(&dlp).unwrap(); }
            if dlr.exists() { std::fs::remove_dir_all(&dlr).unwrap(); }
        } else if use_repair_structure {
            if dlp.exists() { std::fs::remove_dir_all(&dlp).unwrap(); }
            if dlptch.exists() { std::fs::remove_dir_all(&dlptch).unwrap(); }
        } else {
            if dlr.exists() { std::fs::remove_dir_all(&dlr).unwrap(); }
            if dlptch.exists() { std::fs::remove_dir_all(&dlptch).unwrap(); }
        }

        let staging = if use_patching_structure { dlptch.join("staging") } else if use_repair_structure { dlr.join("staging") } else { dlp.join("staging") };
        if !staging.exists() { std::fs::create_dir_all(staging.clone()).unwrap(); }
        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { return false; } }

        let c = AsyncDownloader::setup_client(true).await;
        let dla = AsyncDownloader::new(Arc::new(c), url.as_str()).await;
        if dla.is_err() { return false; }
        let dlu_init = dla.unwrap();
        let file = dlu_init.get_filename().await.to_string();
        let staging_file = staging.join(&file);

        let mut already_verified = false;
        if let Some(vf) = &verified_files {
            let v = vf.lock().unwrap();
            if v.contains(&file) { already_verified = true; }
        }

        if staging_file.exists() {
            let cvalid = if already_verified { true } else if !hash.is_empty() { validate_checksum(staging_file.as_path(), hash.to_ascii_lowercase()).await } else { false };
            if cvalid {
                if !already_verified { if let Some(vf) = &verified_files { vf.lock().unwrap().insert(file.clone()); } }
                let size = staging_file.metadata().map(|m| m.len()).unwrap_or(0);
                progress(size, size, 0, 0);
                return true;
            }
        }

        let progress = Arc::new(Mutex::new(progress));
        let mut failed_chunks: Vec<FailedChunk> = Vec::new();
        let mut last_error = String::new();
        let mut success = false;
        for attempt in 0..3usize {
            if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { break; } }
            let c2 = AsyncDownloader::setup_client(true).await;
            let dla2 = AsyncDownloader::new(Arc::new(c2), url.as_str()).await;
            if let Err(e) = dla2 { last_error = e.to_string(); continue; }
            let mut dlu = dla2.unwrap();
            let progress_c = progress.clone();
            let dl = dlu.download(staging_file.clone(), move |current, total, net_speed, disk_speed| {
                let pl = progress_c.lock().unwrap();
                pl(current, total, net_speed, disk_speed);
            }).await;
            if let Err(e) = &dl { last_error = e.to_string(); continue; }
            if !hash.is_empty() {
                let cvalid = validate_checksum(staging_file.as_path(), hash.to_ascii_lowercase()).await;
                if cvalid {
                    if !already_verified { if let Some(vf) = &verified_files { vf.lock().unwrap().insert(file.clone()); } }
                    success = true;
                    break;
                } else { last_error = format!("Checksum mismatch on attempt {}", attempt + 1); }
            } else {
                success = true;
                break;
            }
        }

        if !success {
            eprintln!("[zipped] failed to download '{}' after 3 retries: {}", file, last_error);
            failed_chunks.push(FailedChunk { file_name: file.clone(), chunk_name: file.clone(), error: last_error });
        }

        if !failed_chunks.is_empty() {
            eprintln!("\n=== Zipped download completed with {} failed file(s) ===", failed_chunks.len());
            for fc in &failed_chunks { eprintln!("  - File: {}, Error: {}", fc.file_name, fc.error); }
            eprintln!("Please run 'Game Repair' after this download completes to fix affected files.\n");
        }
        success
    }

    async fn patch<F>(url: String, hash: String, game_path: String, progress: F, cancel_token: Option<Arc<AtomicBool>>, verified_files: Option<Arc<Mutex<std::collections::HashSet<String>>>>) -> bool where F: Fn(u64, u64, u64, u64, u64, u64, u8) + Send + Sync + 'static {
        if url.is_empty() || game_path.is_empty() { return false; }

        let chunk_file = Path::new(&url).to_path_buf();
        if !chunk_file.exists() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.join("patching");
        let dlp = mainp.join("downloading");
        let dlr = mainp.join("repairing");
        if dlp.exists() { let _ = std::fs::remove_dir_all(&dlp); }
        if dlr.exists() { let _ = std::fs::remove_dir_all(&dlr); }

        let staging = p.join("staging");
        if !staging.exists() { tokio::fs::create_dir_all(&staging).await.unwrap(); }
        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { return false; } }

        let file_name = chunk_file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let already_verified = verified_files.as_ref().map(|vf| vf.lock().unwrap().contains(&file_name)).unwrap_or(false);
        if !already_verified {
            if !hash.is_empty() { if !validate_checksum(chunk_file.as_path(), hash.to_ascii_lowercase()).await { return false; } }
            if let Some(vf) = &verified_files { vf.lock().unwrap().insert(file_name.clone()); }
        }

        if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { return false; } }

        let progress = Arc::new(progress);
        let install_counter = Arc::new(AtomicU64::new(0));
        let install_total_arc = Arc::new(AtomicU64::new(0));

        let monitor_handle = tokio::spawn({
            let install_counter = install_counter.clone();
            let install_total_arc = install_total_arc.clone();
            let progress = progress.clone();
            async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let ic = install_counter.load(Ordering::SeqCst);
                    let it = install_total_arc.load(Ordering::SeqCst);
                    progress(0, 0, ic, it, 0, 0, 3);
                }
            }
        });

        let merged_zip = chunk_file.with_extension("");
        if merged_zip.exists() { let _ = std::fs::remove_file(&merged_zip); }

        {
            let last_reported = Arc::new(AtomicU64::new(0));
            let ic = install_counter.clone();
            let it = install_total_arc.clone();
            extract_archive_with_progress(url.clone(), staging.to_str().unwrap().to_string(), false, move |done, total| {
                it.store(total, Ordering::SeqCst);
                let prev = last_reported.load(Ordering::Relaxed);
                let delta = done.saturating_sub(prev);
                if delta > 0 { last_reported.store(done, Ordering::Relaxed); ic.fetch_add(delta, Ordering::SeqCst); }
            });
        }
        let merged_zip = chunk_file.with_extension("");
        if merged_zip.exists() { let _ = std::fs::remove_file(&merged_zip); }

        let extraction_total = install_total_arc.load(Ordering::SeqCst);
        let already_credited = install_counter.load(Ordering::SeqCst);
        let remainder = extraction_total.saturating_sub(already_credited);
        if remainder > 0 { install_counter.fetch_add(remainder, Ordering::SeqCst); }

        let manifest_file = staging.join("patch.json");
        if !manifest_file.exists() { monitor_handle.abort(); return false; }
        let content = match std::fs::read_to_string(&manifest_file) { Ok(c) => c, Err(_) => { monitor_handle.abort(); return false; } };
        let index: BeyondPatchIndex = match serde_json::from_str(&content) { Ok(i) => i, Err(_) => { monitor_handle.abort(); return false; } };
        let vfs_base = index.vfs_base_path.clone();
        let patch_total: u64 = index.files.iter().filter(|f| f.patch.is_some()).map(|f| f.size).sum();
        let install_total = install_total_arc.fetch_add(patch_total, Ordering::SeqCst) + patch_total;

        let vfs_files_src = staging.join("vfs_files").join("files");
        if vfs_files_src.exists() { let _ = move_all(vfs_files_src.as_ref(), staging.as_ref()).await; }

        let vfs_patch_dir = staging.join("vfs_files").join("vfs_patch");
        let diff_dir = if vfs_patch_dir.exists() {
            std::fs::read_dir(&vfs_patch_dir).ok().and_then(|mut rd| rd.find_map(|e| {
                let e = e.ok()?;
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("diff_") && e.path().is_dir() { Some(name) } else { None }
            }))
        } else { None };

        for fe in index.files.iter().filter(|f| f.patch.is_some()) {
            if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { monitor_handle.abort(); return false; } }
            let output_path = staging.join(&vfs_base).join(&fe.name);
            if let Some(parent) = output_path.parent() { let _ = std::fs::create_dir_all(parent); }
            let entries = fe.patch.as_ref().unwrap();
            let mut patched = false;
            for entry in entries.iter().filter(|e| diff_dir.as_deref().map_or(true, |d| e.patch.starts_with(&format!("{}/", d)))) {
                if let Some(token) = &cancel_token { if token.load(Ordering::Relaxed) { monitor_handle.abort(); return false; } }
                let base_path = mainp.join(&vfs_base).join(&entry.base_file);
                let patch_file = vfs_patch_dir.join(&entry.patch);
                if !base_path.exists() || !patch_file.exists() { continue; }
                let mut hdiff = HDiff::new(base_path.to_str().unwrap().to_string(), patch_file.to_str().unwrap().to_string(), output_path.to_str().unwrap().to_string());
                if hdiff.apply() { patched = true; break; } else { eprintln!("HDiff apply failed for {}", fe.name); }
            }
            if patched { let v = install_counter.fetch_add(fe.size, Ordering::SeqCst) + fe.size; progress(0, 0, v, install_total, 0, 0, 3); } else if !entries.is_empty() { eprintln!("No patch applied for {}", fe.name); }
        }

        monitor_handle.abort();
        progress(0, 0, install_total, install_total, 0, 0, 5);

        let delete_list = staging.join("delete_files.txt");
        if delete_list.exists() {
            if let Ok(content) = std::fs::read_to_string(&delete_list) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    let target = mainp.join(line);
                    if target.exists() { let _ = std::fs::remove_file(&target); }
                }
            }
        }

        let _ = tokio::fs::remove_dir_all(staging.join("vfs_files")).await;
        let _ = tokio::fs::remove_file(staging.join("patch.json")).await;
        let _ = tokio::fs::remove_file(staging.join("delete_files.txt")).await;
        let moved = move_all(staging.as_ref(), mainp.as_ref()).await;
        if moved.is_ok() { let _ = tokio::fs::remove_dir_all(&p).await; }
        true
    }

    async fn repair_game(_res_list: String, _game_path: String, _is_fast: bool, _progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, _cancel_token: Option<Arc<AtomicBool>>, _verified_files: Option<Arc<Mutex<std::collections::HashSet<String>>>>) -> bool { true }
}
