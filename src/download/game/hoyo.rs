use std::io::SeekFrom;
use std::path::{Path};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use async_compression::tokio::bufread::ZstdDecoder;
use futures_util::{stream, StreamExt};
use prost::Message;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use crate::download::game::{Game, Hoyo, Sophon};
use crate::utils::{move_all, patch, validate_checksum};
use crate::utils::downloader::{AsyncDownloader, Downloader};
use crate::utils::game::list_integrity_files;
use crate::utils::proto::{DeleteFiles, PatchChunk, SophonDiff, SophonManifest};

impl Hoyo for Game {
    fn download(urls: Vec<String>, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if urls.is_empty() || game_path.is_empty() {
            return false;
        }

        let progress = Arc::new(Mutex::new(progress));
        for url in urls {
            let p = progress.clone();
            let mut downloader = Downloader::new(url).unwrap();
            let file = downloader.get_filename().to_string();
            downloader.download(Path::new(game_path.as_str()).to_path_buf().join(&file), move |current, total| {
                let pl = p.lock().unwrap();
                pl(current, total);
            }).unwrap();

        }
        true
    }

    fn patch(url: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if url.is_empty() || game_path.is_empty() {
            return false;
        }

        let mut downloader = Downloader::new(url).unwrap();
        let file = downloader.get_filename().to_string();
        let dl = downloader.download(Path::new(game_path.as_str()).to_path_buf().join(&file), progress);

        if dl.is_ok() {
            true
        } else {
            false
        }
    }

    fn repair_game(res_list: String, game_path: String, is_fast: bool, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        let files = list_integrity_files(res_list, "pkg_version".parse().unwrap());

        if files.is_some() {
            let f = files.unwrap();
            let progress = Arc::new(Mutex::new(progress));

            f.iter().for_each(|file| {
                let p = progress.clone();
                let path = Path::new(game_path.as_str());

                if is_fast {
                    let rslt= file.fast_verify(path.to_path_buf().clone());
                    if !rslt {
                        file.repair(path.to_path_buf(), move |current, total| {
                            let pl = p.lock().unwrap();
                            pl(current, total);
                        });
                    }
                } else {
                    let rslt = file.verify(path.to_path_buf().clone());
                    if !rslt {
                        file.repair(path.to_path_buf(), move |current, total| {
                            let pl = p.lock().unwrap();
                            pl(current, total);
                        });
                    }
                }
            });
            true
        } else {
            false
        }
    }

    fn repair_audio(res_list: String, locale: String, game_path: String, is_fast: bool, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        let files = list_integrity_files(res_list, format!("Audio_{}_pkg_version", locale));

        if files.is_some() {
            let f = files.unwrap();
            let progress = Arc::new(Mutex::new(progress));

            f.iter().for_each(|file| {
                let p = progress.clone();
                let path = Path::new(game_path.as_str());

                if is_fast {
                    let rslt = file.fast_verify(path.to_path_buf().clone());
                    if !rslt {
                        file.repair(path.to_path_buf(), move |current, total| {
                            let pl = p.lock().unwrap();
                            pl(current, total);
                        });
                    }
                } else {
                    let rslt = file.verify(path.to_path_buf().clone());
                    if !rslt {
                        file.repair(path.to_path_buf(), move |current, total| {
                            let pl = p.lock().unwrap();
                            pl(current, total);
                        });
                    }
                }
            });
            true
        } else {
            false
        }
    }
}

impl Sophon for Game {
    async fn download(manifest: String, chunk_base: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let p = Path::new(game_path.as_str()).to_path_buf().join("downloading");

        let mut dl = AsyncDownloader::new(manifest).await.unwrap();
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _| {}).await;

            if dll.is_ok() {
                let m = tokio::fs::File::open(p.join(&file).as_path()).await.unwrap();
                let out = tokio::fs::File::create(p.join("manifest").as_path()).await.unwrap();
                let mut decoder = ZstdDecoder::new(tokio::io::BufReader::new(m));
                let mut writer = tokio::io::BufWriter::new(out);
                let rslt = tokio::io::copy(&mut decoder, &mut writer).await;

                if rslt.is_ok() {
                    writer.flush().await.unwrap();

                    let mut f = tokio::fs::File::open(p.join("manifest").as_path()).await.unwrap();
                    let mut file_contents = Vec::new();
                    f.read_to_end(&mut file_contents).await.unwrap();

                    if p.join(&file).exists() { tokio::fs::remove_file(p.join(&file).as_path()).await.unwrap(); }
                    let chunks = p.join("chunk");
                    let staging = p.join("staging");

                    if !chunks.exists() { tokio::fs::create_dir_all(chunks.clone()).await.unwrap(); }
                    if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }
                    let decoded = tokio::task::spawn_blocking(move || {
                        SophonManifest::decode(&mut std::io::Cursor::new(&file_contents)).unwrap()
                    }).await.unwrap();

                    let total_bytes: u64 = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| f.size as u64).sum();
                    let progress_counter = Arc::new(AtomicU64::new(0));

                    let file_semaphore = Arc::new(tokio::sync::Semaphore::new(80));
                    let mut file_futures = Vec::new();
                    let progress = Arc::new(Mutex::new(progress));

                    let chunk_semaphore = Arc::new(tokio::sync::Semaphore::new(180));

                    for file in decoded.files.clone() {
                        let file_semaphore = file_semaphore.clone();
                        let chunk_semaphore = chunk_semaphore.clone();
                        let progress_counter = progress_counter.clone();
                        let progress = progress.clone();

                        let spc = staging.clone();
                        let chunkpp = chunks.clone();
                        let cb = chunk_base.clone();
                        let f = Arc::new(file.clone());

                        let ffut = tokio::task::spawn(async move {
                            let _permit = file_semaphore.acquire().await.unwrap();

                            if f.r#type == 64 { return; }

                            let progress = progress.clone();
                            let progress_counter = progress_counter.clone();
                            let output_path = spc.join(&file.name.clone());
                            let valid = validate_checksum(output_path.as_path(), file.clone().md5.to_ascii_lowercase()).await;

                            // File exists in "staging" directory and checksum is valid skip it
                            // NOTE: This in theory will never be needed, but it is implemented to prevent redownload of already valid files as a form of "catching up"
                            if output_path.exists() && valid { return; } else {
                                if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                            }

                            let chunk_semaphore = chunk_semaphore.clone();
                            let chunks_list = Arc::new(tokio::sync::Mutex::new(Vec::new()));
                            let mut chunk_futures = Vec::new();

                            for (index, chunk) in f.chunks.iter().enumerate() {
                                let chunk_semaphore = chunk_semaphore.clone();

                                let cc = chunk.clone();
                                let file = f.clone();
                                let i = index.clone();

                                let chunkpp = chunkpp.clone();
                                let cb = cb.clone();
                                let file = file.clone();
                                let chunks_list = chunks_list.clone();

                                let fut = tokio::task::spawn(async move {
                                    let _chunk_permit = chunk_semaphore.acquire().await.unwrap();
                                    let cn = cc.chunk_name.clone();
                                    let chunkp = chunkpp.join(cn.clone());

                                    let mut dl = AsyncDownloader::new(format!("{cb}/{cn}").to_string()).await.unwrap();
                                    let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                    if dlf.is_ok() && chunkp.exists() {
                                        let fname = file.name.clone().split("/").last().unwrap_or(file.name.clone().as_str()).to_string() + "_" + &*i.to_string() + ".chunk";
                                        let extc = chunkpp.join(&fname);

                                        let c = tokio::fs::OpenOptions::new().write(true).read(true).open(chunkp.as_path()).await.unwrap();
                                        let out = tokio::fs::File::create(extc.as_path()).await.unwrap();
                                        let mut decoder = ZstdDecoder::new(tokio::io::BufReader::new(c));
                                        let mut writer = tokio::io::BufWriter::new(out);
                                        let rslt = tokio::io::copy(&mut decoder, &mut writer).await;

                                        // Validate decompressed chunks
                                        if rslt.is_ok() {
                                            writer.flush().await.unwrap();
                                            let r = extc.metadata().unwrap().len() as i64 == cc.chunk_decompressed_size;
                                            if r {
                                                let mut list = chunks_list.lock().await;
                                                list.push((i, extc.clone(), cc.chunk_on_file_offset as u64));
                                                tokio::fs::remove_file(chunkp.as_path()).await.unwrap();
                                            }
                                        }

                                    }
                                });
                                chunk_futures.push(fut);
                            }
                            futures_util::future::join_all(chunk_futures).await;

                            // Chunk assembling
                            let mut output = tokio::fs::OpenOptions::new().write(true).create(true).open(&output_path).await.unwrap();
                            output.set_len(file.size as u64).await.unwrap();

                            let mut list = chunks_list.lock().await;
                            // Sort by chunk index
                            list.sort_by_key(|(i, _, _)| *i);

                            for (_i, extc, offset) in list.iter() {
                                let mut chunk_file = tokio::fs::File::open(extc).await.unwrap();
                                let mut buffer = Vec::new();
                                chunk_file.read_to_end(&mut buffer).await.unwrap();
                                output.seek(SeekFrom::Start(*offset)).await.unwrap();
                                output.write_all(&buffer).await.unwrap();
                                tokio::fs::remove_file(extc).await.unwrap();
                            }
                            drop(output);
                            list.clear();

                            let r2 = output_path.metadata().unwrap().len() as i64 == file.size;
                            if r2 {
                                progress_counter.fetch_add(file.size as u64, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                let p = progress.lock().unwrap();
                                p(processed, total_bytes);
                            }
                        });
                        file_futures.push(ffut);
                    }
                    futures_util::future::join_all(file_futures).await;

                    // Validate files
                    let concurrency = 10;
                    stream::iter(decoded.files).map(|f| {
                            let stag = staging.clone();
                            let fullf = stag.join(&f.name);
                            let md5 = f.md5.to_ascii_lowercase();
                            async move {
                                let valid = validate_checksum(fullf.as_path(), md5).await;
                                (f.name, valid)
                            }
                        }).buffer_unordered(concurrency).for_each(|(filename, valid)| async move {
                            if !valid { println!("Invalid checksum for file: {}", filename); }
                        }).await;
                    // Move from "staging" to "game_path" and delete "downloading" directory
                    let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                    if moved.is_ok() { tokio::fs::remove_dir_all(p.as_path()).await.unwrap(); }
                    true
                } else {
                    false
                }
            } else {
                false
            }
    }

    async fn patch(manifest: String, version: String, chunk_base: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.join("patching");

        let mut dl = AsyncDownloader::new(manifest).await.unwrap();
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _| {}).await;

        if dll.is_ok() {
            let m = tokio::fs::File::open(p.join(&file).as_path()).await.unwrap();
            let out = tokio::fs::File::create(p.join("manifest").as_path()).await.unwrap();
            let mut decoder = ZstdDecoder::new(tokio::io::BufReader::new(m));
            let mut writer = tokio::io::BufWriter::new(out);
            let rslt = tokio::io::copy(&mut decoder, &mut writer).await;

            if rslt.is_ok() {
                writer.flush().await.unwrap();

                let mut f = tokio::fs::File::open(p.join("manifest").as_path()).await.unwrap();
                let mut file_contents = Vec::new();
                f.read_to_end(&mut file_contents).await.unwrap();

                if p.join(&file).exists() { tokio::fs::remove_file(p.join(&file).as_path()).await.unwrap(); }
                let chunks = p.join("chunk");
                let staging = p.join("staging");

                if !chunks.exists() { tokio::fs::create_dir_all(chunks.clone()).await.unwrap(); }
                if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || {
                    SophonDiff::decode(&mut std::io::Cursor::new(&file_contents)).unwrap()
                }).await.unwrap();

                let total_bytes: u64 = decoded.files.iter().map(|f| f.size).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));

                for file in decoded.files {
                    let file = Arc::new(file.clone());

                    let output_path = staging.join(&file.name.clone());
                    let valid = validate_checksum(output_path.as_path(), file.clone().md5.to_ascii_lowercase()).await;

                    // File exists in "staging" directory and checksum is valid skip it
                    // NOTE: This in theory will never be needed, but it is implemented to prevent redownload of already valid files as a form of "catching up"
                    if output_path.exists() && valid { continue; } else {
                        if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                    }

                    let chunks_list = Arc::new(tokio::sync::Mutex::new(Vec::new()));
                    let filtered: Vec<(String, PatchChunk)> = file.chunks.clone().into_iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).collect();

                    // File has patches to apply
                    if !filtered.is_empty() {
                        for (_v, chunk) in filtered.into_iter() {
                            let output_path = output_path.clone();

                            let pn = chunk.patch_name;
                            let chunkp = chunks.join(pn.clone());
                            let diffp = chunks.join(format!("{}.hdiff", chunk.patch_md5));
                            let tmpp = chunks.join(format!("{}.tmp", chunk.patch_md5));

                            let mut dl = AsyncDownloader::new(format!("{chunk_base}/{pn}").to_string()).await.unwrap();
                            let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                            if dlf.is_ok() && chunkp.exists() {
                                let r = chunkp.metadata().unwrap().len() == chunk.patch_size;
                                if r {
                                    let mut list = chunks_list.lock().await;
                                    list.push(diffp.clone());
                                    list.push(chunkp.clone());
                                }

                                if chunk.original_filename.is_empty() {
                                    // Chunk is not a hdiff patchable, copy it over
                                    let mut output = tokio::fs::File::create(&tmpp).await.unwrap();
                                    let mut chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                    chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                    let mut r = chunk_file.take(chunk.patch_length);
                                    let mut buffer = vec![0u8; chunk.patch_length as usize];

                                    r.read_exact(&mut buffer).await.unwrap();
                                    output.write_all(&buffer).await.unwrap();
                                    drop(output);

                                    let mut tmpfile = tokio::fs::File::open(&tmpp).await.unwrap();
                                    let mut filed = tokio::fs::File::create(&output_path).await.unwrap();
                                    tokio::io::copy(&mut tmpfile, &mut filed).await.unwrap();
                                } else {
                                    // Chunk is hdiff patchable, patch it
                                    let mut output = tokio::fs::File::create(&diffp).await.unwrap();
                                    let mut chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                    chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                    let mut r = chunk_file.take(chunk.patch_length);
                                    let mut buffer = vec![0u8; chunk.patch_length as usize];

                                    r.read_exact(&mut buffer).await.unwrap();
                                    output.write_all(&buffer).await.unwrap();
                                    drop(output);

                                    // Apply hdiff
                                    // PS: User needs hdiffpatch installed on their system otherwise it won't work for now
                                    let of = mainp.join(&chunk.original_filename);
                                    tokio::task::spawn_blocking(move || {
                                        if let Err(_) = patch(&of, &diffp, &output_path) {}
                                    });
                                }
                            }
                        }
                    }
                    // end chunks
                    let r2 = validate_checksum(output_path.as_path(), file.md5.to_ascii_lowercase()).await;
                    if r2 {
                        let mut list = chunks_list.lock().await;
                        for c in list.clone() {
                            if c.exists() { tokio::fs::remove_file(c).await.unwrap(); }
                        }
                        list.clear();

                        progress_counter.fetch_add(file.size as u64, Ordering::SeqCst);
                        let processed = progress_counter.load(Ordering::SeqCst);
                        progress(processed, total_bytes);
                    }
                }
                // Move from "staging" to "game_path" and delete "patching" directory
                let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                if moved.is_ok() {
                    // Delete all unneeded files after applying the patch and purging the temp directory
                    tokio::fs::remove_dir_all(p.as_path()).await.unwrap();
                    let purge_list: Vec<(String, DeleteFiles)> = decoded.delete_files.into_iter().filter(|(v, _f)| version.as_str() == v.as_str()).collect();

                    if !purge_list.is_empty() {
                        for (_v, df) in purge_list.into_iter() {
                            for f in df.files {
                                let fp = mainp.join(&f.name);
                                if fp.exists() { tokio::fs::remove_file(&fp).await.unwrap(); }
                            }
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

    async fn repair_game(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str());
        let p = mainp.to_path_buf().join("repairing");

        let mut dl = AsyncDownloader::new(manifest).await.unwrap();
        let file = dl.get_filename().await.to_string();
        let dll = dl.download(p.clone().join(&file), |_, _| {}).await;

        if dll.is_ok() {
            let m = tokio::fs::File::open(p.join(&file).as_path()).await.unwrap();
            let out = tokio::fs::File::create(p.join("manifest").as_path()).await.unwrap();
            let mut decoder = ZstdDecoder::new(tokio::io::BufReader::new(m));
            let mut writer = tokio::io::BufWriter::new(out);
            let rslt = tokio::io::copy(&mut decoder, &mut writer).await;

            if rslt.is_ok() {
                writer.flush().await.unwrap();

                let mut f = tokio::fs::File::open(p.join("manifest").as_path()).await.unwrap();
                let mut file_contents = Vec::new();
                f.read_to_end(&mut file_contents).await.unwrap();

                if p.join(&file).exists() { tokio::fs::remove_file(p.join(&file).as_path()).await.unwrap(); }
                let chunks = p.join("chunk");
                let staging = p.join("staging");

                if !chunks.exists() { tokio::fs::create_dir_all(chunks.clone()).await.unwrap(); }
                if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || {
                    SophonManifest::decode(&mut std::io::Cursor::new(&file_contents)).unwrap()
                }).await.unwrap();

                let total_bytes: u64 = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| f.size as u64).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));

                let repaired = Vec::new();
                let file_semaphore = Arc::new(tokio::sync::Semaphore::new(50));
                let mut file_futures = Vec::new();
                let progress = Arc::new(Mutex::new(progress));

                let chunk_semaphore = Arc::new(tokio::sync::Semaphore::new(180));

                for ff in decoded.files.clone() {
                    let file_semaphore = file_semaphore.clone();
                    let chunk_semaphore = chunk_semaphore.clone();

                    let spc = staging.clone();
                    let output = mainp.join(&ff.name);
                    let chunkpp = chunks.clone();
                    let cb = chunk_base.clone();
                    let repaired = repaired.clone();

                    let progress = progress.clone();
                    let progress_counter = progress_counter.clone();

                    let ffut = tokio::task::spawn(async move {
                        let _permit = file_semaphore.acquire().await.unwrap();
                        let progress = progress.clone();
                        let progress_counter = progress_counter.clone();
                        let mut repaired = repaired.clone();

                        if ff.r#type == 64 { return; }

                        if !output.exists() {
                            let staged = spc.join(&ff.name);
                            if staged.exists() { return; } else {
                                if let Some(parent) = staged.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                            }

                            let chunks_list = Arc::new(tokio::sync::Mutex::new(Vec::new()));
                            let mut chunk_futures = Vec::new();

                            for (i, chunk) in ff.chunks.iter().enumerate() {
                                let chunk_semaphore = chunk_semaphore.clone();
                                let cc = chunk.clone();
                                let chunkpp = chunkpp.clone();
                                let cb = cb.clone();
                                let chunks_list = chunks_list.clone();

                                let fut = tokio::task::spawn(async move {
                                    let _chunk_permit = chunk_semaphore.acquire().await.unwrap();
                                    let cn = cc.chunk_name.clone();
                                    let chunkp = chunkpp.join(cn.clone());

                                    let mut dl = AsyncDownloader::new(format!("{cb}/{cn}").to_string()).await.unwrap();
                                    let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                    if dlf.is_ok() && chunkp.exists() {
                                        let fname = cn + ".chunk";
                                        let extc = chunkpp.join(&fname);

                                        let c = tokio::fs::File::open(chunkp.as_path()).await.unwrap();
                                        let out = tokio::fs::File::create(extc.as_path()).await.unwrap();
                                        let mut decoder = ZstdDecoder::new(tokio::io::BufReader::new(c));
                                        let mut writer = tokio::io::BufWriter::new(out);
                                        let rslt = tokio::io::copy(&mut decoder, &mut writer).await;

                                        // Validate decompressed chunks
                                        if rslt.is_ok() {
                                            writer.flush().await.unwrap();
                                            let r = extc.metadata().unwrap().len() as i64 == cc.chunk_decompressed_size;
                                            if r {
                                                let mut list = chunks_list.lock().await;
                                                list.push((i, extc.clone(), cc.chunk_on_file_offset as u64));
                                                tokio::fs::remove_file(chunkp.as_path()).await.unwrap();
                                            }
                                        }
                                    }
                                });
                                chunk_futures.push(fut);
                            }
                            futures_util::future::join_all(chunk_futures).await;

                            // Chunk assembling
                            let mut output = tokio::fs::OpenOptions::new().write(true).create(true).open(&output).await.unwrap();
                            output.set_len(ff.size as u64).await.unwrap();

                            let mut list = chunks_list.lock().await;
                            // Sort by chunk index
                            list.sort_by_key(|(i, _, _)| *i);

                            for (_i, extc, offset) in list.iter() {
                                let mut chunk_file = tokio::fs::File::open(extc).await.unwrap();
                                let mut buffer = Vec::new();
                                chunk_file.read_to_end(&mut buffer).await.unwrap();
                                output.seek(SeekFrom::Start(*offset)).await.unwrap();
                                output.write_all(&buffer).await.unwrap();
                                tokio::fs::remove_file(extc).await.unwrap();
                            }
                            list.clear();

                            let r2 = output.metadata().await.unwrap().len() as i64 == ff.size;
                            if r2 {
                                let p = progress.lock().unwrap();
                                progress_counter.fetch_add(ff.size as u64, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                p(processed, total_bytes);
                                repaired.push((ff.name.clone(), ff.md5.clone()));
                            }
                            drop(output);
                        } else {
                            let valid = if is_fast { output.metadata().unwrap().len() == ff.size as u64 } else { validate_checksum(&output, ff.md5.clone()).await };
                            if !valid {
                                let staged = spc.join(&ff.name);
                                if staged.exists() { return; } else {
                                    if let Some(parent) = staged.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                                }

                                let chunk_semaphore = Arc::new(tokio::sync::Semaphore::new(180));
                                let chunks_list = Arc::new(tokio::sync::Mutex::new(Vec::new()));
                                let mut chunk_futures = Vec::new();

                                for (i, chunk) in ff.chunks.iter().enumerate() {
                                    let chunk_semaphore = chunk_semaphore.clone();
                                    let cc = chunk.clone();
                                    let chunkpp = chunkpp.clone();
                                    let cb = cb.clone();
                                    let chunks_list = chunks_list.clone();

                                    let fut = tokio::task::spawn(async move {
                                        let _chunk_permit = chunk_semaphore.acquire().await.unwrap();
                                        let cn = cc.chunk_name.clone();
                                        let chunkp = chunkpp.join(cn.clone());

                                        let mut dl = AsyncDownloader::new(format!("{cb}/{cn}").to_string()).await.unwrap();
                                        let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                        if dlf.is_ok() {
                                            let fname = cn + ".chunk";
                                            let extc = chunkpp.join(&fname);

                                            let c = tokio::fs::File::open(chunkp.as_path()).await.unwrap();
                                            let out = tokio::fs::File::create(extc.as_path()).await.unwrap();
                                            let mut decoder = ZstdDecoder::new(tokio::io::BufReader::new(c));
                                            let mut writer = tokio::io::BufWriter::new(out);
                                            let rslt = tokio::io::copy(&mut decoder, &mut writer).await;

                                            // Validate decompressed chunks
                                            if rslt.is_ok() {
                                                writer.flush().await.unwrap();
                                                let r = extc.metadata().unwrap().len() as i64 == cc.chunk_decompressed_size;
                                                if r {
                                                    let mut list = chunks_list.lock().await;
                                                    list.push((i, extc.clone(), cc.chunk_on_file_offset as u64));
                                                    tokio::fs::remove_file(chunkp.as_path()).await.unwrap();
                                                }
                                            }
                                        }
                                    });
                                    chunk_futures.push(fut);
                                }
                                futures_util::future::join_all(chunk_futures).await;

                                // Chunk assembling
                                let mut output = tokio::fs::OpenOptions::new().write(true).create(true).open(&output).await.unwrap();
                                output.set_len(ff.size as u64).await.unwrap();

                                let mut list = chunks_list.lock().await;
                                // Sort by chunk index
                                list.sort_by_key(|(i, _, _)| *i);

                                for (_i, extc, offset) in list.iter() {
                                    let mut chunk_file = tokio::fs::File::open(extc).await.unwrap();
                                    let mut buffer = Vec::new();
                                    chunk_file.read_to_end(&mut buffer).await.unwrap();
                                    output.seek(SeekFrom::Start(*offset)).await.unwrap();
                                    output.write_all(&buffer).await.unwrap();
                                    tokio::fs::remove_file(extc).await.unwrap();
                                }
                                list.clear();

                                let r2 = staged.metadata().unwrap().len() as i64 == ff.size;
                                if r2 {
                                    let p = progress.lock().unwrap();
                                    progress_counter.fetch_add(ff.size as u64, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    p(processed, total_bytes);
                                    repaired.push((ff.name.clone(), ff.md5.clone()));
                                }
                                drop(output);
                            } else { return; }
                        }
                    });
                    file_futures.push(ffut);
                }
                futures_util::future::join_all(file_futures).await;

                // Validate files
                let concurrency = 10;
                stream::iter(repaired.clone()).map(|(file, hash)| {
                    let fullf = mainp.join(&file);
                    async move {
                        let valid = validate_checksum(fullf.as_path(), hash.to_ascii_lowercase()).await;
                        (file, valid)
                    }
                }).buffer_unordered(concurrency).for_each(|(filename, valid)| {
                let value = repaired.clone();
                async move {
                    let mut rep = value.clone();
                    if !valid { println!("Invalid checksum for file: {}", filename); }
                    rep.clear();
                }
                }).await;
                // Move from "staging" to "game_path" and delete "repairing" directory
                let moved = move_all(staging.as_ref(), game_path.as_ref()).await;
                if moved.is_ok() { tokio::fs::remove_dir_all(p.as_path()).await.unwrap(); }
                true
            } else {
                false
            }
        } else { false }
    }
}