use std::fs;
use std::path::Path;
use std::sync::{Arc};
use std::sync::atomic::{AtomicU64, Ordering};
use crossbeam_deque::{Injector, Steal, Worker};
use tokio::io::{AsyncReadExt};
use crate::download::game::{Game, Kuro};
use crate::utils::downloader::{AsyncDownloader};
use crate::utils::{extract_archive, krpatchz, move_all, validate_checksum, KuroIndex, KuroResource};

impl Kuro for Game {
    async fn download<F>(manifest: String, base_url: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || base_url.is_empty() { return false; }

        let p = Path::new(game_path.as_str()).to_path_buf();
        let dlp = p.join("downloading");
        let dlr = p.join("repairing");
        let dlptch = p.join("patching");

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
            let injector = Arc::new(Injector::<KuroResource>::new());
            let mut workers = Vec::new();
            let mut stealers_list = Vec::new();
            for _ in 0..8 { let w = Worker::<KuroResource>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
            let stealers = Arc::new(stealers_list);
            for task in files.resource.into_iter() { injector.push(task); }
            let file_sem = Arc::new(tokio::sync::Semaphore::new(8));

            // Spawn worker tasks
            let mut handles = Vec::with_capacity(8);
            for _i in 0..workers.len() {
                let local_worker = workers.pop().unwrap();
                let stealers = stealers.clone();
                let injector = injector.clone();
                let file_sem = file_sem.clone();

                let stealers = stealers.clone();
                let progress_counter = progress_counter.clone();
                let progress_cb = progress.clone();
                let chunk_base = base_url.clone();
                let staging = staging.clone();
                let client = client.clone();

                let mut retry_tasks = Vec::new();
                let handle = tokio::task::spawn(async move {
                    loop {
                        let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                            for s in stealers.iter() { if let Steal::Success(t) = s.steal() { return Some(t); } }
                            None
                        });
                        let Some(chunk_task) = job else { break; };
                        let permit = file_sem.clone().acquire_owned().await.unwrap();

                        let ct = tokio::spawn({
                            let progress_counter = progress_counter.clone();
                            let progress_cb = progress_cb.clone();
                            let chunk_base = chunk_base.clone();
                            let staging = staging.clone();
                            let client = client.clone();
                            async move {
                                let staging_dir = staging.join(chunk_task.dest.clone());
                                let cvalid = validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await;

                                if staging_dir.exists() && cvalid {
                                    progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress_cb(processed, total_bytes);
                                    return;
                                }

                                let pn = chunk_task.dest.clone();
                                let mut dl = AsyncDownloader::new(client.clone(), format!("{chunk_base}/{pn}").to_string()).await.unwrap();
                                let dlf = dl.download(staging_dir.clone(), |_, _| {}).await;
                                let cvalid = validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await;

                                if dlf.is_ok() && cvalid {
                                    progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress_cb(processed, total_bytes);
                                }
                                drop(permit);
                            }
                        }); // end task
                        retry_tasks.push(ct);
                    }
                    for t in retry_tasks { let _ = t.await; }
                });
                handles.push(handle);
            }
            for handle in handles { let _ = handle.await; }
            // All files are complete make sure we report done just in case
            progress(total_bytes, total_bytes);
            let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
            if moved.is_ok() { tokio::fs::remove_dir_all(dlp.as_path()).await.unwrap(); }
            true
        } else {
            false
        }
    }

    async fn patch<F>(manifest: String, base_resources: String, base_zip: String, game_path: String, krpatchz_path: String, preloaded: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || base_resources.is_empty() || base_zip.is_empty() { return false; }

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

            if preloaded {
                // PGR has krzips extract them if they exist
                if files.zip_infos.is_some() {
                    let zips = files.zip_infos.unwrap();
                    for z in zips {
                        let staging = staging.clone();
                        let zp = staging.join(z.dest.clone());
                        if zp.exists() {
                            let r = extract_archive("".to_string(), zp.to_str().unwrap().to_string(), staging.to_str().unwrap().to_string(), false);
                            if r {
                                let fsize = z.entries.iter().map(|f| f.size).sum();
                                progress_counter.fetch_add(fsize, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                progress(processed, total_bytes);
                            }
                        }
                    }
                }
                // Wuwa has krdiffs apply them if they exist
                if files.patch_infos.is_some() {
                    let diffs = files.patch_infos.unwrap();
                    for d in diffs {
                        let staging = staging.clone();
                        let diffp = staging.join(d.dest.clone());
                        let stringed = diffp.to_str().unwrap().to_string();
                        let krd = krpatchz(krpatchz_path.to_owned(), &game_path, &stringed);
                        if krd.is_ok() {
                            let fsize = d.entries.iter().map(|f| f.size).sum();
                            progress_counter.fetch_add(fsize, Ordering::SeqCst);
                            let processed = progress_counter.load(Ordering::SeqCst);
                            progress(processed, total_bytes);
                        } else { eprintln!("Failed to apply krdiff!") }
                        if diffp.exists() { tokio::fs::remove_file(diffp).await.unwrap(); }
                    }
                }
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
            } else {
                let injector = Arc::new(Injector::<KuroResource>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..8 { let w = Worker::<KuroResource>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
                let stealers = Arc::new(stealers_list);
                for task in files.resource.into_iter() { injector.push(task); }
                let file_sem = Arc::new(tokio::sync::Semaphore::new(8));

                // Spawn worker tasks
                let mut handles = Vec::with_capacity(8);
                for _i in 0..workers.len() {
                    let local_worker = workers.pop().unwrap();
                    let stealers = stealers.clone();
                    let injector = injector.clone();
                    let file_sem = file_sem.clone();

                    let stealers = stealers.clone();
                    let progress_counter = progress_counter.clone();
                    let progress_cb = progress.clone();
                    let chunk_res = base_resources.clone();
                    let chunks_zip = base_zip.clone();
                    let staging = staging.clone();
                    let client = client.clone();

                    let mut retry_tasks = Vec::new();
                    let handle = tokio::task::spawn(async move {
                        loop {
                            let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                                for s in stealers.iter() { if let Steal::Success(t) = s.steal() { return Some(t); } }
                                None
                            });
                            let Some(chunk_task) = job else { break; };
                            let permit = file_sem.clone().acquire_owned().await.unwrap();

                            let ct = tokio::spawn({
                                let progress_counter = progress_counter.clone();
                                let progress_cb = progress_cb.clone();
                                let chunk_res = chunk_res.clone();
                                let chunks_zip = chunks_zip.clone();
                                let staging = staging.clone();
                                let client = client.clone();
                                async move {
                                    let staging_dir = staging.join(chunk_task.dest.clone());
                                    let cvalid = validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await;

                                    if staging_dir.exists() && cvalid {
                                        progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                        let processed = progress_counter.load(Ordering::SeqCst);
                                        progress_cb(processed, total_bytes);
                                        return;
                                    }

                                    let pn = chunk_task.dest.clone();
                                    let chunk_base = if chunk_task.dest.ends_with(".krzip") || chunk_task.dest.ends_with(".krdiff") { chunk_res } else { chunks_zip + "/" };

                                    let mut dl = AsyncDownloader::new(client.clone(), format!("{chunk_base}{pn}").to_string()).await.unwrap();
                                    let dlf = dl.download(staging_dir.clone(), |_, _| {}).await;
                                    let cvalid = validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await;

                                    if dlf.is_ok() && cvalid {
                                        progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                        let processed = progress_counter.load(Ordering::SeqCst);
                                        progress_cb(processed, total_bytes);
                                    }
                                    drop(permit);
                                }
                            }); // end task
                            retry_tasks.push(ct);
                        }
                        for t in retry_tasks { let _ = t.await; }
                    });
                    handles.push(handle);
                }
                for handle in handles { let _ = handle.await; }
                // PGR has krzips extract them if they exist
                if files.zip_infos.is_some() {
                    let zips = files.zip_infos.unwrap();
                    for z in zips {
                        let staging = staging.clone();
                        let zp = staging.join(z.dest.clone());
                        if zp.exists() { extract_archive("".to_string(), zp.to_str().unwrap().to_string(), staging.to_str().unwrap().to_string(), false); }
                    }
                }
                // Wuwa has krdiffs apply them if they exist
                if files.patch_infos.is_some() {
                    let diffs = files.patch_infos.unwrap();
                    for d in diffs {
                        let staging = staging.clone();
                        let diffp = staging.join(d.dest.clone());
                        let stringed = diffp.to_str().unwrap().to_string();
                        let krd = krpatchz(krpatchz_path.to_owned(), &game_path, &stringed);
                        if krd.is_ok() {} else { eprintln!("Failed to apply krdiff!") }
                        if diffp.exists() { tokio::fs::remove_file(diffp).await.unwrap(); }
                    }
                }
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

    async fn repair_game<F>(manifest: String, base_url: String, game_path: String, is_fast: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || base_url.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
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

            // Start of download code
            let injector = Arc::new(Injector::<KuroResource>::new());
            let mut workers = Vec::new();
            let mut stealers_list = Vec::new();
            for _ in 0..8 { let w = Worker::<KuroResource>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
            let stealers = Arc::new(stealers_list);
            for task in files.resource.into_iter() { injector.push(task); }
            let file_sem = Arc::new(tokio::sync::Semaphore::new(8));

            // Spawn worker tasks
            let mut handles = Vec::with_capacity(8);
            for _i in 0..workers.len() {
                let local_worker = workers.pop().unwrap();
                let stealers = stealers.clone();
                let injector = injector.clone();
                let file_sem = file_sem.clone();

                let stealers = stealers.clone();
                let progress_counter = progress_counter.clone();
                let progress_cb = progress.clone();
                let chunk_base = base_url.clone();
                let staging = mainp.clone();
                let client = client.clone();

                let mut retry_tasks = Vec::new();
                let handle = tokio::task::spawn(async move {
                    loop {
                        let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                            for s in stealers.iter() { if let Steal::Success(t) = s.steal() { return Some(t); } }
                            None
                        });
                        let Some(chunk_task) = job else { break; };
                        let permit = file_sem.clone().acquire_owned().await.unwrap();

                        let ct = tokio::spawn({
                            let progress_counter = progress_counter.clone();
                            let progress_cb = progress_cb.clone();
                            let chunk_base = chunk_base.clone();
                            let staging = staging.clone();
                            let client = client.clone();
                            async move {
                                let staging_dir = staging.join(chunk_task.dest.clone());
                                let cvalid = if is_fast { staging_dir.metadata().unwrap().len() == chunk_task.size } else { validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await };

                                if staging_dir.exists() && cvalid {
                                    progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress_cb(processed, total_bytes);
                                    return;
                                }

                                let pn = chunk_task.dest.clone();
                                let mut dl = AsyncDownloader::new(client.clone(), format!("{chunk_base}/{pn}").to_string()).await.unwrap();
                                let dlf = dl.download(staging_dir.clone(), |_, _| {}).await;
                                let cvalid = if is_fast { staging_dir.metadata().unwrap().len() == chunk_task.size } else { validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await };

                                if dlf.is_ok() && cvalid {
                                    progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress_cb(processed, total_bytes);
                                }
                                drop(permit);
                            }
                        }); // end task
                        retry_tasks.push(ct);
                    }
                    for t in retry_tasks { let _ = t.await; }
                });
                handles.push(handle);
            }
            for handle in handles { let _ = handle.await; }
            // All files are complete make sure we report done just in case
            progress(total_bytes, total_bytes);
            if p.exists() { tokio::fs::remove_dir_all(p.as_path()).await.unwrap(); }
            true
        } else {
            false
        }
    }

    async fn preload<F>(manifest: String, base_resources: String, base_zip: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || base_resources.is_empty() || base_zip.is_empty() { return false; }

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
            if !p.join(".preload").exists() { fs::File::create(p.join(".preload")).unwrap(); }
            if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }

            let total_bytes: u64 = files.resource.iter().map(|f| f.size).sum();
            let progress_counter = Arc::new(AtomicU64::new(0));
            let progress = Arc::new(progress);

            // Start of download code
            let injector = Arc::new(Injector::<KuroResource>::new());
            let mut workers = Vec::new();
            let mut stealers_list = Vec::new();
            for _ in 0..8 { let w = Worker::<KuroResource>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
            let stealers = Arc::new(stealers_list);
            for task in files.resource.into_iter() { injector.push(task); }
            let file_sem = Arc::new(tokio::sync::Semaphore::new(8));

            // Spawn worker tasks
            let mut handles = Vec::with_capacity(8);
            for _i in 0..workers.len() {
                let local_worker = workers.pop().unwrap();
                let stealers = stealers.clone();
                let injector = injector.clone();
                let file_sem = file_sem.clone();

                let stealers = stealers.clone();
                let progress_counter = progress_counter.clone();
                let progress_cb = progress.clone();
                let chunk_res = base_resources.clone();
                let chunks_zip = base_zip.clone();
                let staging = staging.clone();
                let client = client.clone();

                let mut retry_tasks = Vec::new();
                let handle = tokio::task::spawn(async move {
                    loop {
                        let job = local_worker.pop().or_else(|| injector.steal().success()).or_else(|| {
                            for s in stealers.iter() { if let Steal::Success(t) = s.steal() { return Some(t); } }
                            None
                        });
                        let Some(chunk_task) = job else { break; };
                        let permit = file_sem.clone().acquire_owned().await.unwrap();

                        let ct = tokio::spawn({
                            let progress_counter = progress_counter.clone();
                            let progress_cb = progress_cb.clone();
                            let chunk_res = chunk_res.clone();
                            let chunks_zip = chunks_zip.clone();
                            let staging = staging.clone();
                            let client = client.clone();
                            async move {
                                let staging_dir = staging.join(chunk_task.dest.clone());
                                let cvalid = validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await;

                                if staging_dir.exists() && cvalid {
                                    progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress_cb(processed, total_bytes);
                                    return;
                                }

                                let pn = chunk_task.dest.clone();
                                let chunk_base = if chunk_task.dest.ends_with(".krzip") || chunk_task.dest.ends_with(".krdiff") { chunk_res } else { chunks_zip + "/" };

                                let mut dl = AsyncDownloader::new(client.clone(), format!("{chunk_base}{pn}").to_string()).await.unwrap();
                                let dlf = dl.download(staging_dir.clone(), |_, _| {}).await;
                                let cvalid = validate_checksum(staging_dir.as_path(), chunk_task.md5.to_ascii_lowercase()).await;

                                if dlf.is_ok() && cvalid {
                                    progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress_cb(processed, total_bytes);
                                }
                                drop(permit);
                            }
                        }); // end task
                        retry_tasks.push(ct);
                    }
                    for t in retry_tasks { let _ = t.await; }
                });
                handles.push(handle);
            }
            for handle in handles { let _ = handle.await; }
            // All files are complete make sure we report done just in case
            progress(total_bytes, total_bytes);
            true
        } else { false }
    }
}