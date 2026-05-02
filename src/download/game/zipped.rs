use crate::download::game::{Game, Zipped};
use crate::utils::{FailedChunk, validate_checksum};
use crate::utils::downloader::AsyncDownloader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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

    async fn patch(_url: String, _game_path: String, _progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, _cancel_token: Option<Arc<AtomicBool>>, _verified_files: Option<Arc<Mutex<std::collections::HashSet<String>>>>) -> bool { true }

    async fn repair_game(_res_list: String, _game_path: String, _is_fast: bool, _progress: impl Fn(u64, u64, u64, u64) + Send + Sync + 'static, _cancel_token: Option<Arc<AtomicBool>>, _verified_files: Option<Arc<Mutex<std::collections::HashSet<String>>>>) -> bool { true }
}
