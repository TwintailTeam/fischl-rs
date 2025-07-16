use std::collections::HashSet;
use std::io::{Cursor, SeekFrom};
use std::path::{Path};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use async_compression::tokio::bufread::ZstdDecoder;
use futures_util::StreamExt;
use prost::Message;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use crate::download::game::{Game, Hoyo, Sophon};
use crate::utils::{hpatchz, move_all, validate_checksum};
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
    async fn download<F>(manifest: String, chunk_base: String, game_path: String, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let p = Path::new(game_path.as_str()).to_path_buf().join("downloading");
        let client = Arc::new(AsyncDownloader::setup_client().await);

        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
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
                    drop(writer);

                    let mut f = tokio::fs::OpenOptions::new().read(true).open(p.join("manifest").as_path()).await.unwrap();
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
                    let progress = Arc::new(progress);

                    let file_tasks = futures::stream::iter(decoded.files.into_iter().map(|file| {
                        let chunk_base = chunk_base.clone();
                        let chunkpp = chunks.clone();
                        let staging = staging.clone();
                        let client = client.clone();
                        let progress = progress.clone();
                        let progress_counter = progress_counter.clone();
                        async move {
                            if file.r#type == 64 { return; }

                            let (tx, mut rx) = tokio::sync::mpsc::channel::<(u64, Vec<u8>)>(80);

                            let client = client.clone();
                            let chunkpp = chunkpp.clone();
                            let spc = staging.clone();
                            let output_path = spc.join(file.name.clone());
                            let progress_counter = progress_counter.clone();
                            let progress = progress.clone();
                            let valid = validate_checksum(output_path.as_path(), file.clone().md5.to_ascii_lowercase()).await;

                            // File exists in "staging" directory and checksum is valid skip it
                            // NOTE: This in theory will never be needed, but it is implemented to prevent redownload of already valid files as a form of "catching up"
                            if output_path.exists() && valid {
                                progress_counter.fetch_add(file.size as u64, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                progress(processed, total_bytes);
                                return;
                            } else {
                                if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                            }

                            let to_delete = Arc::new(tokio::sync::Mutex::new(HashSet::new()));
                            let mut output = tokio::fs::File::create(&output_path).await.unwrap();
                            output.set_len(file.size as u64).await.unwrap();

                            let writer_handle = tokio::spawn(async move {
                                while let Some((offset, buffer)) = rx.recv().await {
                                    if let Err(e) = output.seek(SeekFrom::Start(offset)).await { println!("Seek failed: {e}"); break; }
                                    if let Err(e) = output.write_all(&buffer).await { println!("Write failed: {e}"); break; }
                                    if let Err(e) = output.flush().await { println!("Flush failed: {e}"); break; }
                                }
                                drop(output);
                            });

                            let chunk_tasks = futures::stream::iter(file.chunks.into_iter().map(|chunk| {
                                let cb = chunk_base.clone();
                                let chunkpp = chunkpp.clone();
                                let client = client.clone();
                                let to_delete = to_delete.clone();
                                let tx = tx.clone();
                                async move {
                                    let cb = cb.clone();
                                    let cc = chunk.clone();
                                    let tx = tx.clone();
                                    let chunkpp = chunkpp.clone();
                                    let client = client.clone();
                                    let to_delete = to_delete.clone();

                                    let cn = cc.chunk_name.clone();
                                    let chunkp = chunkpp.join(cn.clone());

                                    let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{cn}").to_string()).await.unwrap();
                                    let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                    if dlf.is_ok() && chunkp.exists() {
                                        let c = tokio::fs::File::open(chunkp.as_path()).await.unwrap();
                                        let reader = tokio::io::BufReader::with_capacity(512 * 1024, c);
                                        let mut decoder = ZstdDecoder::new(reader);
                                        let mut buffer = Vec::with_capacity(cc.chunk_size as usize);

                                        if tokio::io::copy(&mut decoder, &mut buffer).await.is_ok() {
                                            if let Err(e) = tx.send((chunk.chunk_on_file_offset as u64, buffer)).await { println!("[ERROR] Failed to send chunk data: {e}"); }
                                            let mut del = to_delete.lock().await;
                                            del.insert(chunkp.clone());
                                            drop(del);
                                        }
                                    } else {
                                        let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{cn}").to_string()).await.unwrap();
                                        let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                        if dlf.is_ok() {
                                            let c = tokio::fs::File::open(chunkp.as_path()).await.unwrap();
                                            let reader = tokio::io::BufReader::with_capacity(512 * 1024, c);
                                            let mut decoder = ZstdDecoder::new(reader);
                                            let mut buffer = Vec::with_capacity(cc.chunk_size as usize);

                                            if tokio::io::copy(&mut decoder, &mut buffer).await.is_ok() {
                                                if let Err(e) = tx.send((chunk.chunk_on_file_offset as u64, buffer)).await { println!("[ERROR] Failed to send chunk data: {e}"); }
                                                let mut del = to_delete.lock().await;
                                                del.insert(chunkp.clone());
                                                drop(del);
                                            }
                                        }
                                    }
                                }
                            })).buffer_unordered(80).collect::<Vec<_>>();
                            chunk_tasks.await;
                            drop(tx);
                            writer_handle.await.ok();

                            let r2 = validate_checksum(output_path.as_path(), file.md5.to_ascii_lowercase()).await;
                            if r2 {
                                progress_counter.fetch_add(file.size as u64, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                progress(processed, total_bytes);
                                let to_delete = to_delete.lock().await;
                                for chunk in to_delete.iter() { tokio::fs::remove_file(&chunk).await.unwrap(); }
                                drop(to_delete);
                            } else {  }
                        }
                    })).buffer_unordered(1).collect::<Vec<_>>();
                    file_tasks.await;
                    // All files are complete make sure we report done just in case
                    progress(total_bytes, total_bytes);
                    // Move from "staging" to "game_path" and delete "downloading" directory
                    let moved = move_all(Path::new(&staging), Path::new(&game_path)).await;
                    if moved.is_ok() { tokio::fs::remove_dir_all(p.as_path()).await.unwrap(); }
                    true
                } else {
                    false
                }
            } else {
                false
            }
    }

    async fn patch(manifest: String, version: String, chunk_base: String, game_path: String, hpatchz_path: String, preloaded: bool, compression: bool, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.join("patching");
        let client = Arc::new(AsyncDownloader::setup_client().await);

        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
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

                if !chunks.exists() && !preloaded { tokio::fs::create_dir_all(chunks.clone()).await.unwrap(); }
                if !staging.exists() && !preloaded { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || {
                    SophonDiff::decode(&mut std::io::Cursor::new(&file_contents)).unwrap()
                }).await.unwrap();

                let total_bytes: u64 = decoded.files.iter().map(|f| f.size).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));

                for file in decoded.files {
                    let file = Arc::new(file.clone());
                    let hpatchz_path = hpatchz_path.clone();
                    let output_path = staging.join(file.name.clone());
                    let valid = validate_checksum(output_path.as_path(), file.clone().md5.to_ascii_lowercase()).await;

                    // File exists in "staging" directory and checksum is valid skip it
                    // NOTE: This in theory will never be needed, but it is implemented to prevent redownload of already valid files as a form of "catching up"
                    if output_path.exists() && valid {
                        progress_counter.fetch_add(file.size, Ordering::SeqCst);
                        let processed = progress_counter.load(Ordering::SeqCst);
                        progress(processed, total_bytes);
                        continue;
                    } else {
                        if let Some(parent) = output_path.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
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
                                            let mut output = tokio::fs::File::create(&output_path).await.unwrap();
                                            let chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                            let reader = tokio::io::BufReader::with_capacity(512 * 1024, chunk_file);
                                            let mut decoder = ZstdDecoder::new(reader);
                                            let mut buffer = Vec::with_capacity(chunk.patch_size as usize);
                                            tokio::io::copy(&mut decoder, &mut buffer).await.unwrap();
                                            let mut cursor = Cursor::new(buffer);

                                            cursor.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                            let mut r = cursor.take(chunk.patch_length);
                                            tokio::io::copy(&mut r, &mut output).await.unwrap();
                                            output.flush().await.unwrap();
                                            drop(output);
                                        } else {
                                            let mut output = tokio::fs::File::create(&output_path).await.unwrap();
                                            let mut chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                            chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                            let mut r = chunk_file.take(chunk.patch_length);
                                            tokio::io::copy(&mut r, &mut output).await.unwrap();
                                            output.flush().await.unwrap();
                                            drop(output);
                                        }
                                    } else {
                                        // Chunk is hdiff patchable, patch it
                                        if compression {
                                            let mut output = tokio::fs::File::create(&diffp).await.unwrap();
                                            let chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                            let reader = tokio::io::BufReader::with_capacity(512 * 1024, chunk_file);
                                            let mut decoder = ZstdDecoder::new(reader);
                                            let mut buffer = Vec::with_capacity(chunk.patch_size as usize);
                                            tokio::io::copy(&mut decoder, &mut buffer).await.unwrap();
                                            let mut cursor = Cursor::new(buffer);

                                            cursor.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                            let mut r = cursor.take(chunk.patch_length);
                                            tokio::io::copy(&mut r, &mut output).await.unwrap();
                                            output.flush().await.unwrap();
                                            drop(output);

                                            let of = mainp.join(&chunk.original_filename);
                                            tokio::task::spawn_blocking(move || {
                                                if let Err(_) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) {}
                                            });
                                        } else {
                                            let mut output = tokio::fs::File::create(&diffp).await.unwrap();
                                            let mut chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                            chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                            let mut r = chunk_file.take(chunk.patch_length);
                                            tokio::io::copy(&mut r, &mut output).await.unwrap();
                                            output.flush().await.unwrap();
                                            drop(output);

                                            let of = mainp.join(&chunk.original_filename);
                                            tokio::task::spawn_blocking(move || {
                                                if let Err(_) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) {}
                                            });
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
                                                let mut output = tokio::fs::File::create(&output_path).await.unwrap();
                                                let chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                                let reader = tokio::io::BufReader::with_capacity(512 * 1024, chunk_file);
                                                let mut decoder = ZstdDecoder::new(reader);
                                                let mut buffer = Vec::with_capacity(chunk.patch_size as usize);
                                                tokio::io::copy(&mut decoder, &mut buffer).await.unwrap();
                                                let mut cursor = Cursor::new(buffer);

                                                cursor.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                                let mut r = cursor.take(chunk.patch_length);
                                                tokio::io::copy(&mut r, &mut output).await.unwrap();
                                                output.flush().await.unwrap();
                                                drop(output);
                                            } else {
                                                let mut output = tokio::fs::File::create(&output_path).await.unwrap();
                                                let mut chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                                chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                                let mut r = chunk_file.take(chunk.patch_length);
                                                tokio::io::copy(&mut r, &mut output).await.unwrap();
                                                output.flush().await.unwrap();
                                                drop(output);
                                            }
                                        } else {
                                            // Chunk is hdiff patchable, patch it
                                            if compression {
                                                let mut output = tokio::fs::File::create(&diffp).await.unwrap();
                                                let chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                                let reader = tokio::io::BufReader::with_capacity(512 * 1024, chunk_file);
                                                let mut decoder = ZstdDecoder::new(reader);
                                                let mut buffer = Vec::with_capacity(chunk.patch_size as usize);
                                                tokio::io::copy(&mut decoder, &mut buffer).await.unwrap();
                                                let mut cursor = Cursor::new(buffer);

                                                cursor.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                                let mut r = cursor.take(chunk.patch_length);
                                                tokio::io::copy(&mut r, &mut output).await.unwrap();
                                                output.flush().await.unwrap();
                                                drop(output);

                                                let of = mainp.join(&chunk.original_filename);
                                                tokio::task::spawn_blocking(move || {
                                                    if let Err(_) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) {}
                                                });
                                            } else {
                                                let mut output = tokio::fs::File::create(&diffp).await.unwrap();
                                                let mut chunk_file = tokio::fs::File::open(chunkp.as_path()).await.unwrap();

                                                chunk_file.seek(SeekFrom::Start(chunk.patch_offset)).await.unwrap();
                                                let mut r = chunk_file.take(chunk.patch_length);
                                                tokio::io::copy(&mut r, &mut output).await.unwrap();
                                                output.flush().await.unwrap();
                                                drop(output);

                                                let of = mainp.join(&chunk.original_filename);
                                                tokio::task::spawn_blocking(move || {
                                                    if let Err(_) = hpatchz(hpatchz_path.to_owned(), &of, &diffp, &output_path) {}
                                                });
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

    async fn repair_game<F>(manifest: String, chunk_base: String, game_path: String, is_fast: bool, progress: F) -> bool where F: Fn(u64, u64) + Send + Sync + 'static {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str());
        let p = mainp.to_path_buf().join("repairing");
        let client = Arc::new(AsyncDownloader::setup_client().await);

        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
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

                if !chunks.exists() { tokio::fs::create_dir_all(chunks.clone()).await.unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || {
                    SophonManifest::decode(&mut std::io::Cursor::new(&file_contents)).unwrap()
                }).await.unwrap();

                let total_bytes: u64 = decoded.files.iter().filter(|f| f.r#type != 64).map(|f| f.size as u64).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));
                let progress = Arc::new(progress);

                let file_tasks = futures::stream::iter(decoded.files.into_iter().map(|ff| {
                    let chunk_base = chunk_base.clone();
                    let chunkpp = chunks.clone();
                    let client = client.clone();
                    let progress = progress.clone();
                    let progress_counter = progress_counter.clone();
                    async move {
                        let mainp = mainp;
                        let outputp = mainp.join(&ff.name);
                        let chunkpp = chunkpp.clone();
                        let cb = chunk_base.clone();
                        let client = client.clone();
                        let progress = progress.clone();
                        let progress_counter = progress_counter.clone();

                        if ff.r#type == 64 { return; }

                        let (tx, mut rx) = tokio::sync::mpsc::channel::<(u64, Vec<u8>)>(80);

                        if !outputp.exists() {
                            if outputp.exists() {
                                tokio::fs::remove_file(&outputp).await.unwrap();
                                if let Some(parent) = outputp.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                            } else {
                                if let Some(parent) = outputp.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                            }

                            let to_delete = Arc::new(tokio::sync::Mutex::new(HashSet::new()));
                            let mut output = tokio::fs::File::create(&outputp).await.unwrap();
                            output.set_len(ff.size as u64).await.unwrap();

                            let writer_handle = tokio::spawn(async move {
                                while let Some((offset, buffer)) = rx.recv().await {
                                    if let Err(e) = output.seek(SeekFrom::Start(offset)).await { println!("Seek failed: {e}"); break; }
                                    if let Err(e) = output.write_all(&buffer).await { println!("Write failed: {e}"); break; }
                                    if let Err(e) = output.flush().await { println!("Flush failed: {e}"); break; }
                                }
                                drop(output);
                            });

                            let chunk_tasks = futures::stream::iter(ff.chunks.into_iter().map(|chunk| {
                                let cb = cb.clone();
                                let chunkpp = chunkpp.clone();
                                let client = client.clone();
                                let to_delete = to_delete.clone();
                                let tx = tx.clone();
                                async move {
                                    let cb = cb.clone();
                                    let cc = chunk.clone();
                                    let tx = tx.clone();
                                    let chunkpp = chunkpp.clone();
                                    let client = client.clone();
                                    let to_delete = to_delete.clone();

                                    let cn = cc.chunk_name.clone();
                                    let chunkp = chunkpp.join(cn.clone());

                                    let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{cn}").to_string()).await.unwrap();
                                    let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                    if dlf.is_ok() && chunkp.exists() {
                                        let c = tokio::fs::File::open(chunkp.as_path()).await.unwrap();
                                        let reader = tokio::io::BufReader::with_capacity(512 * 1024, c);
                                        let mut decoder = ZstdDecoder::new(reader);
                                        let mut buffer = Vec::with_capacity(cc.chunk_size as usize);

                                        if tokio::io::copy(&mut decoder, &mut buffer).await.is_ok() {
                                            if let Err(e) = tx.send((chunk.chunk_on_file_offset as u64, buffer)).await { println!("[ERROR] Failed to send chunk data: {e}"); }
                                            let mut del = to_delete.lock().await;
                                            del.insert(chunkp.clone());
                                            drop(del);
                                        }
                                    } else {
                                        let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{cn}").to_string()).await.unwrap();
                                        let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                        if dlf.is_ok() {
                                            let c = tokio::fs::File::open(chunkp.as_path()).await.unwrap();
                                            let reader = tokio::io::BufReader::with_capacity(512 * 1024, c);
                                            let mut decoder = ZstdDecoder::new(reader);
                                            let mut buffer = Vec::with_capacity(cc.chunk_size as usize);

                                            if tokio::io::copy(&mut decoder, &mut buffer).await.is_ok() {
                                                if let Err(e) = tx.send((chunk.chunk_on_file_offset as u64, buffer)).await { println!("[ERROR] Failed to send chunk data: {e}"); }
                                                let mut del = to_delete.lock().await;
                                                del.insert(chunkp.clone());
                                                drop(del);
                                            }
                                        }
                                    }
                                }
                            })).buffer_unordered(80).collect::<Vec<()>>();
                            chunk_tasks.await;
                            drop(tx);
                            writer_handle.await.ok();

                            let r2 = if is_fast { outputp.metadata().unwrap().len() as i64 == ff.size } else { validate_checksum(outputp.as_path(), ff.md5.to_ascii_lowercase()).await };
                            if r2 {
                                progress_counter.fetch_add(ff.size as u64, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                progress(processed, total_bytes);
                                let to_delete = to_delete.lock().await;
                                for chunk in to_delete.iter() { tokio::fs::remove_file(&chunk).await.unwrap(); }
                                drop(to_delete);
                            } else {  }
                        } else {
                            let valid = if is_fast { outputp.metadata().unwrap().len() == ff.size as u64 } else { validate_checksum(&outputp, ff.md5.to_ascii_lowercase()).await };
                            if !valid {
                                if outputp.exists() {
                                    tokio::fs::remove_file(&outputp).await.unwrap();
                                    if let Some(parent) = outputp.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                                } else {
                                    if let Some(parent) = outputp.parent() { tokio::fs::create_dir_all(parent).await.unwrap(); }
                                }

                                let to_delete = Arc::new(tokio::sync::Mutex::new(HashSet::new()));
                                let mut output = tokio::fs::File::create(&outputp).await.unwrap();
                                output.set_len(ff.size as u64).await.unwrap();

                                let writer_handle = tokio::spawn(async move {
                                    while let Some((offset, buffer)) = rx.recv().await {
                                        if let Err(e) = output.seek(SeekFrom::Start(offset)).await { println!("Seek failed: {e}"); break; }
                                        if let Err(e) = output.write_all(&buffer).await { println!("Write failed: {e}"); break; }
                                        if let Err(e) = output.flush().await { println!("Flush failed: {e}"); break; }
                                    }
                                    drop(output);
                                });

                                let chunk_tasks = futures::stream::iter(ff.chunks.into_iter().map(|chunk| {
                                    let cb = chunk_base.clone();
                                    let chunkpp = chunkpp.clone();
                                    let client = client.clone();
                                    let to_delete = to_delete.clone();
                                    let tx = tx.clone();
                                    async move {
                                        let cb = cb.clone();
                                        let cc = chunk.clone();
                                        let tx = tx.clone();
                                        let chunkpp = chunkpp.clone();
                                        let client = client.clone();
                                        let to_delete = to_delete.clone();

                                        let cn = cc.chunk_name.clone();
                                        let chunkp = chunkpp.join(cn.clone());

                                        let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{cn}").to_string()).await.unwrap();
                                        let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                        if dlf.is_ok() && chunkp.exists() {
                                            let c = tokio::fs::File::open(chunkp.as_path()).await.unwrap();
                                            let reader = tokio::io::BufReader::with_capacity(512 * 1024, c);
                                            let mut decoder = ZstdDecoder::new(reader);
                                            let mut buffer = Vec::with_capacity(cc.chunk_size as usize);

                                            if tokio::io::copy(&mut decoder, &mut buffer).await.is_ok() {
                                                if let Err(e) = tx.send((chunk.chunk_on_file_offset as u64, buffer)).await { println!("[ERROR] Failed to send chunk data: {e}"); }
                                                let mut del = to_delete.lock().await;
                                                del.insert(chunkp.clone());
                                                drop(del);
                                            }
                                        } else {
                                            let mut dl = AsyncDownloader::new(client.clone(), format!("{cb}/{cn}").to_string()).await.unwrap();
                                            let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                            if dlf.is_ok() {
                                                let c = tokio::fs::File::open(chunkp.as_path()).await.unwrap();
                                                let reader = tokio::io::BufReader::with_capacity(512 * 1024, c);
                                                let mut decoder = ZstdDecoder::new(reader);
                                                let mut buffer = Vec::with_capacity(cc.chunk_size as usize);

                                                if tokio::io::copy(&mut decoder, &mut buffer).await.is_ok() {
                                                    if let Err(e) = tx.send((chunk.chunk_on_file_offset as u64, buffer)).await { println!("[ERROR] Failed to send chunk data: {e}"); }
                                                    let mut del = to_delete.lock().await;
                                                    del.insert(chunkp.clone());
                                                    drop(del);
                                                }
                                            }
                                        }
                                    }
                                })).buffer_unordered(80).collect::<Vec<()>>();
                                chunk_tasks.await;
                                drop(tx);
                                writer_handle.await.ok();

                                let r2 = if is_fast { outputp.metadata().unwrap().len() as i64 == ff.size } else { validate_checksum(outputp.as_path(), ff.md5.to_ascii_lowercase()).await };
                                if r2 {
                                    progress_counter.fetch_add(ff.size as u64, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress(processed, total_bytes);
                                    let to_delete = to_delete.lock().await;
                                    for chunk in to_delete.iter() { tokio::fs::remove_file(&chunk).await.unwrap(); }
                                    drop(to_delete);
                                } else {  }
                            } else {
                                progress_counter.fetch_add(ff.size as u64, Ordering::SeqCst);
                                let processed = progress_counter.load(Ordering::SeqCst);
                                progress(processed, total_bytes);
                            }
                        }
                    }
                })).buffer_unordered(1).collect::<Vec<()>>();
                file_tasks.await;
                // All files are complete make sure we report done just in case
                progress(total_bytes, total_bytes);
                if p.exists() { tokio::fs::remove_dir_all(p.as_path()).await.unwrap(); }
                true
            } else {
                false
            }
        } else { false }
    }

    async fn preload(manifest: String, version: String, chunk_base: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        if manifest.is_empty() || game_path.is_empty() || chunk_base.is_empty() { return false; }

        let mainp = Path::new(game_path.as_str()).to_path_buf();
        let p = mainp.join("patching");
        let client = Arc::new(AsyncDownloader::setup_client().await);

        let mut dl = AsyncDownloader::new(client.clone(), manifest).await.unwrap();
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
                if !p.join(".preload").exists() { tokio::fs::File::create(p.join(".preload")).await.unwrap(); }
                let chunks = p.join("chunk");
                let staging = p.join("staging");

                if !chunks.exists() { tokio::fs::create_dir_all(chunks.clone()).await.unwrap(); }
                if !staging.exists() { tokio::fs::create_dir_all(staging.clone()).await.unwrap(); }
                let decoded = tokio::task::spawn_blocking(move || {
                    SophonDiff::decode(&mut std::io::Cursor::new(&file_contents)).unwrap()
                }).await.unwrap();

                let total_bytes: u64 = decoded.files.iter().map(|f| f.size).sum();
                let progress_counter = Arc::new(AtomicU64::new(0));
                let progress = Arc::new(progress);

                let file_tasks = futures::stream::iter(decoded.files.into_iter().map(|file| {
                    let client = client.clone();
                    let chunk_base = chunk_base.clone();
                    let version = version.clone();
                    let chunkp = chunks.clone();
                    let file = file.clone();
                    let progress = progress.clone();
                    let progress_counter = progress_counter.clone();
                    async move {
                        let file = file.clone();
                        let version = version.clone();
                        let chunkp = chunkp.clone();
                        let chunk_base = chunk_base.clone();
                        let filtered: Vec<(String, PatchChunk)> = file.chunks.clone().into_iter().filter(|(v, _chunk)| version.as_str() == v.as_str()).collect();
                        // File has patches to apply
                        if !filtered.is_empty() {
                            for (_v, chunk) in filtered.into_iter() {
                                let pn = chunk.patch_name;
                                let chunkp = chunkp.join(pn.clone());

                                if chunkp.exists() {
                                    progress_counter.fetch_add(file.size, Ordering::SeqCst);
                                    let processed = progress_counter.load(Ordering::SeqCst);
                                    progress(processed, total_bytes);
                                    return;
                                }

                                let mut dl = AsyncDownloader::new(client.clone(), format!("{chunk_base}/{pn}").to_string()).await.unwrap();
                                let dlf = dl.download(chunkp.clone(), |_, _| {}).await;

                                if dlf.is_ok() && chunkp.exists() {
                                    let r = validate_checksum(chunkp.as_path(), chunk.patch_md5.to_ascii_lowercase()).await;
                                    if r {
                                        progress_counter.fetch_add(file.size, Ordering::SeqCst);
                                        let processed = progress_counter.load(Ordering::SeqCst);
                                        progress(processed, total_bytes);
                                    }
                                }
                            }
                        } // end
                    }
                })).buffer_unordered(10).collect::<Vec<()>>();
                file_tasks.await;
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