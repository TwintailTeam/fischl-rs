use std::fs;
use std::io::{Cursor, SeekFrom, Write, Read, BufWriter, BufReader, copy, Seek};
use std::path::{Path, PathBuf};
use std::sync::{Arc};
use std::sync::atomic::{AtomicU64, Ordering};
use crossbeam_deque::{Injector, Steal, Worker};
use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use prost::Message;
use reqwest_middleware::ClientWithMiddleware;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use crate::download::game::{Game, Sophon};
use crate::utils::{hpatchz, move_all, validate_checksum};
use crate::utils::downloader::{AsyncDownloader};
use crate::utils::proto::{DeleteFiles, ManifestFile, PatchChunk, PatchFile, SophonDiff, SophonManifest};

impl Sophon for Game {
    async fn download<F>(manifest: String, chunk_base: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let p = Path::new(game_path.as_str()).to_path_buf();
        let dlp = p.join("downloading");
        let dlr = p.join("repairing");
        let dlptch = p.join("patching");

        if dlr.exists() { fs::remove_dir_all(&dlr).unwrap(); }
        if dlptch.exists() { fs::remove_dir_all(&dlptch).unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client().await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
        let file = dl.get_filename().await.to_string();
        let dlm = dl.download(dlp.clone().join(&file), |_, _| {}).await;

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

                let total_bytes: u64 = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| f.size).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));
                let progress = Arc::new(progress);

                // Start of download code
                let injector = Arc::new(Injector::<ManifestFile>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..8 { let w = Worker::<ManifestFile>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
                let stealers = Arc::new(stealers_list);
                for task in decoded.files.into_iter() { injector.push(task); }
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
                    let chunks_dir = chunks.clone();
                    let staging_dir = staging.clone();
                    let chunk_base = chunk_base.clone();
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
                                let chunks_dir = chunks_dir.clone();
                                let staging_dir = staging_dir.clone();
                                let chunk_base = chunk_base.clone();
                                let client = client.clone();
                                async move {
                                    process_file_chunks(chunk_task.clone(), chunks_dir.clone(), staging_dir.clone(), chunk_base.clone(), client.clone(), false).await;

                                    let fp = staging_dir.join(chunk_task.clone().name);
                                    validate_file(chunk_task.clone(), chunk_base.clone(), chunks_dir.clone(), staging_dir.clone(), fp.clone(), client.clone(), progress_counter.clone(), progress_cb.clone(), total_bytes, false).await;
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
                // Move from "staging" to "game_path" and delete "downloading" directory
                let moved = move_all(Path::new(&staging), Path::new(&game_path)).await;
                if moved.is_ok() { fs::remove_dir_all(dlp.as_path()).unwrap(); }
                true
            } else { false }
        } else {
            false
        }
    }

    async fn patch<F>(manifest: String, version: String, chunk_base: String, game_path: String, hpatchz_path: String, preloaded: bool, compression: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.join("patching");
        let dlp = p.join("downloading");
        let dlr = p.join("repairing");

        if dlp.exists() { fs::remove_dir_all(&dlp).unwrap(); }
        if dlr.exists() { fs::remove_dir_all(&dlr).unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client().await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _| {}).await;

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

                let total_bytes: u64 = decoded.files.iter().map(|f| { f.chunks.iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).map(|(_v, _chunk)| f.size).sum::<u64>() }).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));

                for file in decoded.files {
                    let ff = Arc::new(file.clone());
                    let hpatchz_path = hpatchz_path.clone();
                    let output_path = staging.join(file.name.clone());
                    let valid = validate_checksum(output_path.as_path(), file.clone().md5.to_ascii_lowercase()).await;

                    if output_path.exists() && valid {
                        progress_counter.fetch_add(file.size, Ordering::SeqCst);
                        let processed = progress_counter.load(Ordering::SeqCst);
                        progress(processed, total_bytes);
                        continue;
                    } else {
                        if let Some(parent) = output_path.parent() { fs::create_dir_all(parent).unwrap(); }
                    }

                    let filtered: Vec<(String, PatchChunk)> = file.chunks.clone().into_iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).collect();
                    // File has patches to apply
                    if !filtered.is_empty() {
                        for (_v, chunk) in filtered.into_iter() {
                            let output_path = output_path.clone();
                            let hpatchz_path = hpatchz_path.clone();

                            let pn = chunk.patch_name;
                            let chunkp = chunks.join(pn.clone());
                            let diffp = chunks.join(format!("{}.hdiff", chunk.patch_md5));

                            // User has predownloaded validate each chunk and apply patches
                            if preloaded {
                                let r = validate_checksum(chunkp.as_path(), chunk.patch_md5.to_ascii_lowercase()).await;
                                if r {
                                    if chunk.original_filename.is_empty() {
                                        // Chunk is not a hdiff patchable, copy it over
                                        if compression {
                                            let mut output = fs::File::create(&output_path).unwrap();
                                            let chunk_file = fs::File::open(chunkp.as_path()).unwrap();

                                            let reader = BufReader::with_capacity(512 * 1024, chunk_file);
                                            let mut decoder = zstd::stream::Decoder::new(reader).unwrap();
                                            let mut buffer = Vec::with_capacity(chunk.patch_size as usize);
                                            copy(&mut decoder, &mut buffer).unwrap();
                                            let mut cursor = Cursor::new(buffer);

                                            std::io::Seek::seek(&mut cursor, SeekFrom::Start(chunk.patch_offset)).unwrap();
                                            let mut r = cursor.take(chunk.patch_length);
                                            copy(&mut r, &mut output).unwrap();
                                            output.flush().unwrap();
                                            drop(output);
                                        } else {
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
                                                if !of.exists() { fs::File::create(&of).unwrap(); }
                                                if let Err(e) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) { eprintln!("Failed to hpatchz with error: {}", e);}
                                            } else {
                                                let mut output = fs::File::create(&output_path).unwrap();
                                                let mut cursor = Cursor::new(&r);
                                                copy(&mut cursor, &mut output).unwrap();
                                                output.flush().unwrap();
                                                drop(output);
                                            }
                                        }
                                    } else {
                                        // Chunk is hdiff patchable, patch it
                                        if compression {
                                            let mut output = fs::File::create(&diffp).unwrap();
                                            let chunk_file = fs::File::open(chunkp.as_path()).unwrap();

                                            let reader = BufReader::with_capacity(512 * 1024, chunk_file);
                                            let mut decoder = zstd::stream::Decoder::new(reader).unwrap();
                                            let mut buffer = Vec::with_capacity(chunk.patch_size as usize);
                                            copy(&mut decoder, &mut buffer).unwrap();
                                            let mut cursor = Cursor::new(buffer);

                                            std::io::Seek::seek(&mut cursor, SeekFrom::Start(chunk.patch_offset)).unwrap();
                                            let mut r = cursor.take(chunk.patch_length);
                                            copy(&mut r, &mut output).unwrap();
                                            output.flush().unwrap();
                                            drop(output);

                                            let of = mainp.join(&chunk.original_filename);
                                            if let Err(e) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) { eprintln!("Failed to hpatchz with error: {}", e);}
                                        } else {
                                            let mut output = fs::File::create(&diffp).unwrap();
                                            let mut chunk_file = fs::File::open(chunkp.as_path()).unwrap();

                                            chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).unwrap();
                                            let mut r = chunk_file.take(chunk.patch_length);
                                            copy(&mut r, &mut output).unwrap();
                                            output.flush().unwrap();
                                            drop(output);

                                            let of = mainp.join(&chunk.original_filename);
                                            if let Err(e) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) { eprintln!("Failed to hpatchz with error: {}", e);}
                                        }
                                    }
                                } else { continue; }
                            } else {
                                let mut dl = AsyncDownloader::new(client.clone(), format!("{chunk_base}/{pn}").to_string()).await.unwrap();
                                let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                if dlf.is_ok() && chunkp.exists() {
                                    let r = validate_checksum(chunkp.as_path(), chunk.patch_md5.to_ascii_lowercase()).await;
                                    if r {
                                        if chunk.original_filename.is_empty() {
                                            // Chunk is not a hdiff patchable, copy it over
                                            if compression {
                                                let mut output = fs::File::create(&output_path).unwrap();
                                                let chunk_file = fs::File::open(chunkp.as_path()).unwrap();

                                                let reader = BufReader::with_capacity(512 * 1024, chunk_file);
                                                let mut decoder = zstd::stream::Decoder::new(reader).unwrap();
                                                let mut buffer = Vec::with_capacity(chunk.patch_size as usize);
                                                copy(&mut decoder, &mut buffer).unwrap();
                                                let mut cursor = Cursor::new(buffer);

                                                std::io::Seek::seek(&mut cursor, SeekFrom::Start(chunk.patch_offset)).unwrap();
                                                let mut r = cursor.take(chunk.patch_length);
                                                copy(&mut r, &mut output).unwrap();
                                                output.flush().unwrap();
                                                drop(output);
                                            } else {
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
                                                    if !of.exists() { fs::File::create(&of).unwrap(); }
                                                    if let Err(e) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) { eprintln!("Failed to hpatchz with error: {}", e);}
                                                } else {
                                                    let mut output = fs::File::create(&output_path).unwrap();
                                                    let mut cursor = Cursor::new(&r);
                                                    copy(&mut cursor, &mut output).unwrap();
                                                    output.flush().unwrap();
                                                    drop(output);
                                                }
                                                /*let mut output = fs::File::create(&output_path).unwrap();
                                                let mut chunk_file = fs::File::open(chunkp.as_path()).unwrap();

                                                chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).unwrap();
                                                let mut r = chunk_file.take(chunk.patch_length);
                                                copy(&mut r, &mut output).unwrap();
                                                output.flush().unwrap();
                                                drop(output);*/
                                            }
                                        } else {
                                            // Chunk is hdiff patchable, patch it
                                            if compression {
                                                let mut output = fs::File::create(&diffp).unwrap();
                                                let chunk_file = fs::File::open(chunkp.as_path()).unwrap();

                                                let reader = BufReader::with_capacity(512 * 1024, chunk_file);
                                                let mut decoder = zstd::stream::Decoder::new(reader).unwrap();
                                                let mut buffer = Vec::with_capacity(chunk.patch_size as usize);
                                                copy(&mut decoder, &mut buffer).unwrap();
                                                let mut cursor = Cursor::new(buffer);

                                                std::io::Seek::seek(&mut cursor, SeekFrom::Start(chunk.patch_offset)).unwrap();
                                                let mut r = cursor.take(chunk.patch_length);
                                                copy(&mut r, &mut output).unwrap();
                                                output.flush().unwrap();
                                                drop(output);

                                                let of = mainp.join(&chunk.original_filename);
                                                if let Err(e) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) { eprintln!("Failed to hpatchz with error: {}", e);}
                                            } else {
                                                let mut output = fs::File::create(&diffp).unwrap();
                                                let mut chunk_file = fs::File::open(chunkp.as_path()).unwrap();

                                                chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).unwrap();
                                                let mut r = chunk_file.take(chunk.patch_length);
                                                copy(&mut r, &mut output).unwrap();
                                                output.flush().unwrap();
                                                drop(output);

                                                let of = mainp.join(&chunk.original_filename);
                                                if let Err(e) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) { eprintln!("Failed to hpatchz with error: {}", e);}
                                            }
                                        }
                                    } else { continue; }
                                }
                            } // preload check end
                        }
                        // end chunks
                        let r2 = validate_checksum(output_path.as_path(), file.md5.to_ascii_lowercase()).await;
                        if r2 {
                            progress_counter.fetch_add(file.size, Ordering::SeqCst);
                            let processed = progress_counter.load(Ordering::SeqCst);
                            progress(processed, total_bytes);
                        }
                    } else { continue; }
                }
                // All files are complete make sure we report done just in case
                progress(total_bytes, total_bytes);
                let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                if moved.is_ok() {
                    // Delete all unneeded files after applying the patch and purging the temp directory
                    if preloaded {  } else { fs::remove_dir_all(p.as_path()).unwrap(); }
                    let purge_list: Vec<(String, DeleteFiles)> = decoded.delete_files.into_iter().filter(|(v, _f)| version.as_str() == v.as_str()).collect();
                    if !purge_list.is_empty() {
                        for (_v, df) in purge_list.into_iter() { for f in df.files { let fp = mainp.join(&f.name); if fp.exists() { fs::remove_file(&fp).unwrap(); }; }
                        }
                    }
                }
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    async fn repair_game<F>(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str());
        let mainpbuf = mainp.to_path_buf();
        let p = mainpbuf.join("repairing");
        let dlp = p.join("downloading");
        let dlptch = p.join("patching");

        if dlp.exists() { fs::remove_dir_all(&dlp).unwrap(); }
        if dlptch.exists() { fs::remove_dir_all(&dlptch).unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client().await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _| {}).await;

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
                let decoded = tokio::task::spawn_blocking(move || { SophonManifest::decode(&mut std::io::Cursor::new(&file_contents)).unwrap() }).await.unwrap();

                let total_bytes: u64 = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| f.size).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));
                let progress = Arc::new(progress);

                // Start of download code
                let injector = Arc::new(Injector::<ManifestFile>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..8 { let w = Worker::<ManifestFile>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
                let stealers = Arc::new(stealers_list);
                for task in decoded.files.into_iter() { injector.push(task); }
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
                    let chunks_dir = chunks.clone();
                    let mainp = mainpbuf.clone();
                    let chunk_base = chunk_base.clone();
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
                                let mainp = mainp.clone();
                                let chunks_dir = chunks_dir.clone();
                                let chunk_base = chunk_base.clone();
                                let client = client.clone();
                                async move {
                                    process_file_chunks(chunk_task.clone(), chunks_dir.clone(), mainp.clone(), chunk_base.clone(), client.clone(), is_fast).await;

                                    let fp = mainp.join(chunk_task.clone().name);
                                    validate_file(chunk_task.clone(), chunk_base.clone(), chunks_dir.clone(), mainp.clone(), fp.clone(), client.clone(), progress_counter.clone(), progress_cb.clone(), total_bytes, is_fast).await;
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
                if p.exists() { fs::remove_dir_all(p.as_path()).unwrap(); }
                true
            } else {
                false
            }
        } else { false }
    }

    async fn preload<F>(manifest: String, version: String, chunk_base: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.join("patching");
        let dlr = mainp.join("repairing");
        let dlp = mainp.join("downloading");

        // If these directories exist delete them for safety
        if dlr.exists() { fs::remove_dir_all(&dlr).unwrap(); }
        if dlp.exists() { fs::remove_dir_all(&dlp).unwrap(); }

        let client = Arc::new(AsyncDownloader::setup_client().await);
        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _| {}).await;

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
                let staging = p.join("staging");

                if !chunks.exists() { fs::create_dir_all(chunks.clone()).unwrap(); }
                if !staging.exists() { fs::create_dir_all(staging.clone()).unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || { SophonDiff::decode(&mut Cursor::new(&file_contents)).unwrap() }).await.unwrap();

                let total_bytes: u64 = decoded.files.iter().map(|f| { f.chunks.iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).map(|(_v, _chunk)| f.size).sum::<u64>() }).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));
                let progress = Arc::new(progress);

                // Start of download code
                let injector = Arc::new(Injector::<PatchFile>::new());
                let mut workers = Vec::new();
                let mut stealers_list = Vec::new();
                for _ in 0..8 { let w = Worker::<PatchFile>::new_fifo();stealers_list.push(w.stealer());workers.push(w); }
                let stealers = Arc::new(stealers_list);
                for task in decoded.files.into_iter() { injector.push(task); }
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
                    let chunks_dir = chunks.clone();
                    let chunk_base = chunk_base.clone();
                    let version = version.clone();
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
                                let chunks_dir = chunks_dir.clone();
                                let chunk_base = chunk_base.clone();
                                let version = version.clone();
                                let client = client.clone();
                                async move {
                                    let filtered: Vec<(String, PatchChunk)> = chunk_task.chunks.clone().into_iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).collect();

                                    // File has patches to apply
                                    if !filtered.is_empty() {
                                        for (_v, chunk) in filtered.into_iter() {
                                            let pn = chunk.patch_name;
                                            let chunkp = chunks_dir.join(pn.clone());

                                            if chunkp.exists() {
                                                progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                                let processed = progress_counter.load(Ordering::SeqCst);
                                                progress_cb(processed, total_bytes);
                                                return;
                                            }

                                            let mut dl = AsyncDownloader::new(client.clone(), format!("{chunk_base}/{pn}").to_string()).await.unwrap();
                                            let dlf = dl.download(chunkp.clone(), |_, _| {}).await;
                                            let cvalid = validate_checksum(chunkp.as_path(), chunk.patch_md5.to_ascii_lowercase()).await;

                                            if dlf.is_ok() && cvalid {
                                                progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                                                let processed = progress_counter.load(Ordering::SeqCst);
                                                progress_cb(processed, total_bytes);
                                            }
                                        }
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
            } else {
                false
            }
        } else {
            false
        }
    }
}

async fn process_file_chunks(chunk_task: ManifestFile, chunks_dir: PathBuf, staging_dir: PathBuf, chunk_base: String, client: Arc<ClientWithMiddleware>, is_fast: bool) {
    if chunk_task.r#type == 64 { return; }

    let fp = staging_dir.join(&chunk_task.name);
    let validstg = if is_fast { fp.metadata().unwrap().len() == chunk_task.size } else { validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await };
    if fp.exists() && validstg { return; } else {
        if let Some(parent) = fp.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
    }

    let file = tokio::fs::OpenOptions::new().create(true).write(true).open(&fp).await.unwrap();
    file.set_len(chunk_task.size).await.unwrap();
    let writer = tokio::sync::Mutex::new(tokio::io::BufWriter::new(file));
    let csem = Arc::new(tokio::sync::Semaphore::new(200));

    let mut chunk_futures = FuturesUnordered::new();
    for c in chunk_task.chunks.clone() {
        let chunk_path = chunks_dir.join(&c.chunk_name);
        let client = Arc::clone(&client);
        let chunk_base = chunk_base.clone();
        let sem = csem.clone();

        chunk_futures.push(async move {
            let mut dl = AsyncDownloader::new(client, format!("{}/{}", chunk_base, c.chunk_name)).await.unwrap();
            let dl_result = dl.download(chunk_path.clone(), |_, _| {}).await;

            if dl_result.is_ok() {
                let valid = validate_checksum(chunk_path.as_path(), c.chunk_md5.to_ascii_lowercase()).await;
                if valid && chunk_path.exists() {
                    let p = sem.acquire_owned().await.unwrap();
                    let buffer = tokio::task::spawn_blocking(move || {
                        let file = fs::File::open(&chunk_path).unwrap();
                        let mut reader = BufReader::with_capacity(512 * 1024, file);
                        let mut decoder = zstd::Decoder::new(&mut reader).unwrap();
                        let mut buf = Vec::with_capacity(c.chunk_decompressed_size as usize);
                        copy(&mut decoder, &mut buf).unwrap();
                        buf
                    }).await.unwrap();
                    drop(p);
                    return Some((buffer, c.chunk_on_file_offset));
                }
            }
            None
        });
    }
    let mut writer = writer.lock().await;
    while let Some(opt) = chunk_futures.next().await {
        if let Some((buffer, offset)) = opt {
            writer.seek(SeekFrom::Start(offset)).await.unwrap();
            writer.write_all(&buffer).await.unwrap();
        }
    }
    writer.flush().await.unwrap();
    drop(writer);
}

async fn validate_file<F>(chunk_task: ManifestFile, chunk_base: String, chunks_dir: PathBuf, staging_dir: PathBuf, fp: PathBuf, client: Arc<ClientWithMiddleware>, progress_counter: Arc<AtomicU64>, progress_cb: Arc<F>, total_bytes: u64, is_fast: bool) where F: Fn(u64, u64) + Send + Sync + 'static {
    let valid = validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
    if !valid {
        if fp.exists() { if let Err(e) = tokio::fs::remove_file(&fp).await { eprintln!("Failed to delete incomplete file before retry: {}: {}", fp.display(), e); } }
        process_file_chunks(chunk_task.clone(), chunks_dir.clone(), staging_dir.clone(), chunk_base.clone(), client.clone(), is_fast).await;
        let revalid = validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
        if !revalid {
            if fp.exists() { if let Err(e) = tokio::fs::remove_file(&fp).await { eprintln!("Failed to delete incomplete file before re-retry: {}: {}", fp.display(), e); } }
            process_file_chunks(chunk_task.clone(), chunks_dir.clone(), staging_dir.clone(), chunk_base.clone(), client.clone(), is_fast).await;
            let revalid2 = validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
            if !revalid2 {
                if fp.exists() { if let Err(e) = tokio::fs::remove_file(&fp).await { eprintln!("Failed to delete incomplete file before re-re-retry: {}: {}", fp.display(), e); } }
                process_file_chunks(chunk_task.clone(), chunks_dir.clone(), staging_dir.clone(), chunk_base.clone(), client.clone(), is_fast).await;
                let revalid3 = validate_checksum(fp.as_path(), chunk_task.md5.to_ascii_lowercase()).await;
                if !revalid3 { eprintln!("Failed to validate file after 3 retries! Please run game repair after finishing. Affected file: {}", chunk_task.name.clone()); } else {
                    let processed = progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                    progress_cb(processed, total_bytes);
                    for c in &chunk_task.chunks {
                        let chunk_path = chunks_dir.join(&c.chunk_name);
                        if chunk_path.exists() { if let Err(e) = tokio::fs::remove_file(&chunk_path).await { eprintln!("Failed to delete chunk file {}: {}", chunk_path.display(), e); } }
                    }
                }
            } else {
                let processed = progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
                progress_cb(processed, total_bytes);
                for c in &chunk_task.chunks {
                    let chunk_path = chunks_dir.join(&c.chunk_name);
                    if chunk_path.exists() { if let Err(e) = tokio::fs::remove_file(&chunk_path).await { eprintln!("Failed to delete chunk file {}: {}", chunk_path.display(), e); } }
                }
            }
        } else {
            let processed = progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
            progress_cb(processed, total_bytes);
            for c in &chunk_task.chunks {
                let chunk_path = chunks_dir.join(&c.chunk_name);
                if chunk_path.exists() { if let Err(e) = tokio::fs::remove_file(&chunk_path).await { eprintln!("Failed to delete chunk file {}: {}", chunk_path.display(), e); } }
            }
        }
    } else {
        let processed = progress_counter.fetch_add(chunk_task.size, Ordering::SeqCst);
        progress_cb(processed, total_bytes);
        for c in &chunk_task.chunks {
            let chunk_path = chunks_dir.join(&c.chunk_name);
            if chunk_path.exists() { if let Err(e) = tokio::fs::remove_file(&chunk_path).await { eprintln!("Failed to delete chunk file {}: {}", chunk_path.display(), e); } }
        }
    }
}