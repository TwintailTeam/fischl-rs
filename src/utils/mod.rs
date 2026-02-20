use std::{fs, io};
use std::hash::{BuildHasher, Hasher};
use std::io::{Error, Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use crate::utils::github_structs::GithubRelease;

pub(crate) mod github_structs;
pub(crate) mod proto;
pub mod downloader;
pub mod free_space;
pub use downloader::SpeedTracker;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FailedChunk {
    pub file_name: String,
    pub chunk_name: String,
    pub error: String,
}

pub fn get_github_release(repository: String) -> Option<GithubRelease> {
    if repository.is_empty() {
        None
    } else {
        let url = format!("https://api.github.com/repos/{}/releases/latest", repository);
        let client = reqwest::blocking::Client::new();
        let response = client.get(url).header(USER_AGENT, "lib/fischl-rs").header("X-GitHub-Api-Version", "2022-11-28").header("Accept", "application/vnd.github+json").send();
        match response {
            Ok(resp) => {
                if let Err(err) = resp.error_for_status_ref() { eprintln!("GitHub API returned error status: {:?}", err.status());return None; }
                match resp.json::<GithubRelease>() {
                    Ok(github_release) => Some(github_release),
                    Err(json_err) => { eprintln!("Failed to parse JSON: {}", json_err);None }
                }
            }
            Err(err) => { eprintln!("Network or request failed: {}", err);None }
        }
    }
}

pub fn extract_archive_with_progress<F>(archive_path: String, extract_dest: String, move_subdirs: bool, progress_callback: F) -> bool where F: Fn(u64, u64) + Send + 'static {
    let src = Path::new(&archive_path);
    let dest = Path::new(&extract_dest);

    if !src.exists() {
        false
    } else {
        if !dest.exists() { fs::create_dir_all(dest).unwrap(); }
        actually_uncompress_with_progress(src.to_str().unwrap().to_string(), dest.to_str().unwrap().to_string(), move_subdirs, progress_callback);
        if src.exists() { fs::remove_file(src).unwrap(); }
        true
    }
}

pub(crate) fn move_all<'a>(src: &'a Path, dst: &'a Path) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if !dst.exists() { tokio::fs::create_dir_all(dst).await?; }

        let mut dir = tokio::fs::read_dir(src).await?;
        while let Some(entry) = dir.next_entry().await? {
            let entry_path = entry.path();
            let dest_path = dst.join(entry.file_name());
            let ty = entry.file_type().await?;

            if ty.is_dir() {
                move_all(&entry_path, &dest_path).await?;
                tokio::fs::remove_dir_all(&entry_path).await?;
            } else if ty.is_file() {
                tokio::fs::rename(&entry_path, &dest_path).await?;
            }
        }
        Ok(())
    })
}

pub(crate) async fn count_dir_bytes(path: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    let mut dir = tokio::fs::read_dir(path).await?;
    while let Some(entry) = dir.next_entry().await? {
        let ty = entry.file_type().await?;
        if ty.is_dir() {
            total += Box::pin(count_dir_bytes(&entry.path())).await?;
        } else if ty.is_file() {
            total += entry.metadata().await?.len();
        }
    }
    Ok(total)
}

pub(crate) async fn move_all_with_progress<F>(src: &Path, dst: &Path, total_bytes: u64, progress: &F) -> io::Result<u64> where F: Fn(u64, u64) + Send + Sync {
    let mut moved_bytes = 0u64;
    if !dst.exists() { tokio::fs::create_dir_all(dst).await?; }

    let mut dir = tokio::fs::read_dir(src).await?;
    while let Some(entry) = dir.next_entry().await? {
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        let ty = entry.file_type().await?;

        if ty.is_dir() {
            moved_bytes += Box::pin(move_all_with_progress(&entry_path, &dest_path, total_bytes, progress)).await?;
            tokio::fs::remove_dir_all(&entry_path).await?;
        } else if ty.is_file() {
            let file_size = entry.metadata().await?.len();
            tokio::fs::rename(&entry_path, &dest_path).await?;
            moved_bytes += file_size;
            progress(moved_bytes, total_bytes);
        }
    }
    Ok(moved_bytes)
}

pub(crate) async fn validate_checksum(file: &Path, checksum: String) -> bool {
    match tokio::fs::File::open(file).await {
        Ok(f) => {
            match chksum_md5::async_chksum(f).await {
                Ok(digest) => digest.to_hex_lowercase() == checksum.to_ascii_lowercase(),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

#[inline]
pub fn prettify_bytes(bytes: u64) -> String {
    if bytes > 1000 * 1000 * 1000 {
        format!("{:.2} GB", bytes as f64 / 1000.0 / 1000.0 / 1000.0)
    } else if bytes > 1000 * 1000 {
        format!("{:.2} MB", bytes as f64 / 1000.0 / 1000.0)
    } else if bytes > 1000 {
        format!("{:.2} KB", bytes as f64 / 1000.0)
    } else {
        format!("{:.2} B", bytes)
    }
}

pub fn is_process_running(process_name: &str) -> bool {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let pns = process_name.split('.').collect::<Vec<&str>>();
    let pn = pns.first().unwrap_or(&process_name);
    sys.processes().values().any(|process| {
        if let Some(apn) = process.name().to_str() {
            let apns = apn.split('.').collect::<Vec<&str>>();
            if let Some(apnn) = apns.first() { return apnn.contains(pn); }
        }
        false
    })
}

pub fn wait_for_process<F>(process_name: &str, delay_ms: u64, retries: usize, mut callback: F) -> bool where F: FnMut(bool) -> bool {
    for _ in 0..retries {
        let found = is_process_running(process_name);
        if callback(found) { return found; }
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }
    false
}

pub fn hpatchz<T: Into<PathBuf> + std::fmt::Debug>(bin_path: String, file: T, patch: T, output: T) -> io::Result<()> {
    let output = Command::new(bin_path.as_str()).arg("-f").arg(file.into().as_os_str()).arg(patch.into().as_os_str()).arg(output.into().as_os_str()).output()?;

    if String::from_utf8_lossy(output.stdout.as_slice()).contains("patch ok!") { Ok(()) } else {
        let err = String::from_utf8_lossy(&output.stderr);
        Err(Error::other(format!("Failed to apply hdiff patch: {err}")))
    }
}

pub(crate) fn actually_uncompress_with_progress<F>(archive_path: String, dest: String, strip_head_path: bool, progress_callback: F) where F: Fn(u64, u64) + Send + 'static {
    let ext = get_full_extension(archive_path.as_str()).unwrap();
    match ext {
        "zip" | "krzip" => {
            let file = match fs::File::open(&archive_path) { Ok(f) => f, Err(_) => return };
            let mut archive = match zip::ZipArchive::new(file) { Ok(a) => a, Err(_) => return };
            let dest_path = Path::new(&dest);

            let mut top_prefix: Option<PathBuf> = None;
            let mut prefix_found = !strip_head_path;
            let mut total_size: u64 = 0;
            let mut work_items: Vec<(usize, PathBuf, u64, Option<u32>)> = Vec::new();
            let mut dir_modes: Vec<(PathBuf, u32)> = Vec::new();

            for i in 0..archive.len() {
                if let Ok(f) = archive.by_index(i) {
                    let file_size = f.size();
                    total_size += file_size;
                    let unix_mode = f.unix_mode();
                    let safe_name = match f.enclosed_name() { Some(p) => p, None => continue };

                    if !prefix_found { if let Some(first) = safe_name.components().next() { top_prefix = Some(PathBuf::from(first.as_os_str())); }prefix_found = true; }
                    let out_path = if let Some(ref top) = top_prefix { match safe_name.strip_prefix(top) { Ok(rel) if !rel.as_os_str().is_empty() => dest_path.join(rel), _ => continue, } } else { dest_path.join(&safe_name) };
                    if f.is_dir() {
                        fs::create_dir_all(&out_path).unwrap_or(());
                        #[cfg(unix)]
                        if let Some(mode) = unix_mode { dir_modes.push((out_path, mode & 0o7777)); }
                    } else {
                        if let Some(parent) = out_path.parent() { fs::create_dir_all(parent).unwrap_or(()); }
                        work_items.push((i, out_path, file_size, unix_mode));
                    }
                }
            }
            drop(archive);

            work_items.sort_unstable_by_key(|(_, _, sz, _)| *sz);
            let work = Arc::new(Mutex::new(work_items));
            let extracted = Arc::new(AtomicU64::new(0));
            let last_report = Arc::new(AtomicU64::new(0));
            const REPORT_EVERY: u64 = 4 * 1024 * 1024;
            let cb = Arc::new(Mutex::new(progress_callback));
            let archive_path = Arc::new(archive_path);

            let mut handles = Vec::with_capacity(10);
            for _ in 0..10 {
                let work = Arc::clone(&work);
                let extracted = Arc::clone(&extracted);
                let last_report = Arc::clone(&last_report);
                let cb = Arc::clone(&cb);
                let ap = Arc::clone(&archive_path);

                handles.push(std::thread::spawn(move || {
                    let file = match fs::File::open(ap.as_str()) { Ok(f) => f, Err(_) => return };
                    let mut archive = match zip::ZipArchive::new(file) { Ok(a) => a, Err(_) => return };
                    let mut buf = vec![0u8; 2 * 1024 * 1024];

                    loop {
                        let item = work.lock().unwrap().pop();
                        let (idx, out_path, file_size, unix_mode) = match item { Some(v) => v, None => break };
                        let mut f = match archive.by_index(idx) { Ok(f) => f, Err(_) => { extracted.fetch_add(file_size, Ordering::Relaxed); continue; } };
                        let mut out_file = match fs::File::create(&out_path) { Ok(f) => f, Err(_) => { extracted.fetch_add(file_size, Ordering::Relaxed); continue; } };

                        if file_size > 0 { out_file.set_len(file_size).unwrap_or(()); }
                        loop {
                            let n = match f.read(&mut buf) { Ok(n) => n, Err(_) => break };
                            if n == 0 { break; }
                            if out_file.write_all(&buf[..n]).is_err() { break; }
                            let done = extracted.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
                            let prev = last_report.load(Ordering::Relaxed);
                            if done.saturating_sub(prev) >= REPORT_EVERY && last_report.compare_exchange(prev, done, Ordering::Relaxed, Ordering::Relaxed).is_ok() { cb.lock().unwrap()(done, total_size); }
                        }
                        #[cfg(unix)]
                        if let Some(mode) = unix_mode {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(&out_path, fs::Permissions::from_mode(mode & 0o7777)).unwrap_or(());
                        }
                    }
                }));
            }
            for h in handles { h.join().unwrap_or(()); }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                for (path, mode) in dir_modes.into_iter().rev() { fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap_or(()); }
            }
            cb.lock().unwrap()(extracted.load(Ordering::Relaxed), total_size);
        },
        "tar.gz" => {
            let total_size: u64 = fs::File::open(&archive_path).ok().and_then(|f| {
                let mut arc = tar::Archive::new(flate2::read::GzDecoder::new(f));
                arc.entries().ok().map(|es| { es.filter_map(|e| e.ok()).filter(|e| e.header().entry_type().is_file()).map(|e| e.header().size().unwrap_or(0)).sum() })
            }).unwrap_or(0);
            let file = match fs::File::open(&archive_path) { Ok(f) => f, Err(_) => return };
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
            let dest_path = Path::new(&dest);
            let mut top_prefix: Option<PathBuf> = None;
            let mut prefix_found = !strip_head_path;
            let mut extracted: u64 = 0;
            let mut last_report: u64 = 0;
            const REPORT_EVERY: u64 = 4 * 1024 * 1024;
            #[cfg(unix)] let mut dir_modes: Vec<(PathBuf, u32)> = Vec::new();

            let entries = match archive.entries() { Ok(e) => e, Err(_) => return };
            for entry_res in entries {
                let mut entry = match entry_res { Ok(e) => e, Err(_) => continue };
                let raw_path = match entry.path() { Ok(p) => p.to_path_buf(), Err(_) => continue };
                let raw_path = raw_path.strip_prefix(".").unwrap_or(&raw_path).to_path_buf();
                if !prefix_found {
                    if let Some(first) = raw_path.components().next() { top_prefix = Some(PathBuf::from(first.as_os_str())); }
                    prefix_found = true;
                }
                let out_path = if let Some(ref top) = top_prefix { match raw_path.strip_prefix(top) { Ok(rel) if !rel.as_os_str().is_empty() => dest_path.join(rel), _ => continue, } } else { dest_path.join(&raw_path) };
                let kind = entry.header().entry_type();
                if kind.is_dir() {
                    fs::create_dir_all(&out_path).unwrap_or(());
                    #[cfg(unix)] if let Ok(mode) = entry.header().mode() { dir_modes.push((out_path, mode & 0o7777)); }
                } else if kind.is_file() {
                    let file_size = entry.header().size().unwrap_or(0);
                    if let Some(parent) = out_path.parent() { fs::create_dir_all(parent).unwrap_or(()); }
                    let mut out_file = match fs::File::create(&out_path) { Ok(f) => f, Err(_) => continue };
                    if file_size > 0 { out_file.set_len(file_size).unwrap_or(()); }
                    let mut buf = [0u8; 256 * 1024];
                    loop {
                        let n = match entry.read(&mut buf) { Ok(n) => n, Err(_) => break };
                        if n == 0 { break; }
                        if out_file.write_all(&buf[..n]).is_err() { break; }
                        extracted += n as u64;
                        if extracted.saturating_sub(last_report) >= REPORT_EVERY { last_report = extracted; progress_callback(extracted, total_size); }
                    }
                    #[cfg(unix)] if let Ok(mode) = entry.header().mode() { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&out_path, fs::Permissions::from_mode(mode & 0o7777)).unwrap_or(()); }
                } else {
                    if let Some(parent) = out_path.parent() { fs::create_dir_all(parent).unwrap_or(()); }
                    let _ = entry.unpack(&out_path);
                }
            }
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; for (path, mode) in dir_modes.into_iter().rev() { fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap_or(()); } }
            progress_callback(extracted, total_size);
        },
        "tar.xz" => {
            let total_size: u64 = fs::File::open(&archive_path).ok().and_then(|f| {
                let mut arc = tar::Archive::new(liblzma::read::XzDecoder::new(f));
                arc.entries().ok().map(|es| { es.filter_map(|e| e.ok()).filter(|e| e.header().entry_type().is_file()).map(|e| e.header().size().unwrap_or(0)).sum() })
                }).unwrap_or(0);
            let file = match fs::File::open(&archive_path) { Ok(f) => f, Err(_) => return };
            let mut archive = tar::Archive::new(liblzma::read::XzDecoder::new(file));
            let dest_path = Path::new(&dest);
            let mut top_prefix: Option<PathBuf> = None;
            let mut prefix_found = !strip_head_path;
            let mut extracted: u64 = 0;
            let mut last_report: u64 = 0;
            const REPORT_EVERY: u64 = 4 * 1024 * 1024;
            #[cfg(unix)] let mut dir_modes: Vec<(PathBuf, u32)> = Vec::new();

            let entries = match archive.entries() { Ok(e) => e, Err(_) => return };
            for entry_res in entries {
                let mut entry = match entry_res { Ok(e) => e, Err(_) => continue };
                let raw_path = match entry.path() { Ok(p) => p.to_path_buf(), Err(_) => continue };
                let raw_path = raw_path.strip_prefix(".").unwrap_or(&raw_path).to_path_buf();
                if !prefix_found {
                    if let Some(first) = raw_path.components().next() { top_prefix = Some(PathBuf::from(first.as_os_str())); }
                    prefix_found = true;
                }
                let out_path = if let Some(ref top) = top_prefix { match raw_path.strip_prefix(top) { Ok(rel) if !rel.as_os_str().is_empty() => dest_path.join(rel), _ => continue, } } else { dest_path.join(&raw_path) };
                let kind = entry.header().entry_type();
                if kind.is_dir() {
                    fs::create_dir_all(&out_path).unwrap_or(());
                    #[cfg(unix)] if let Ok(mode) = entry.header().mode() { dir_modes.push((out_path, mode & 0o7777)); }
                } else if kind.is_file() {
                    let file_size = entry.header().size().unwrap_or(0);
                    if let Some(parent) = out_path.parent() { fs::create_dir_all(parent).unwrap_or(()); }
                    let mut out_file = match fs::File::create(&out_path) { Ok(f) => f, Err(_) => continue };
                    if file_size > 0 { out_file.set_len(file_size).unwrap_or(()); }
                    let mut buf = [0u8; 256 * 1024];
                    loop {
                        let n = match entry.read(&mut buf) { Ok(n) => n, Err(_) => break };
                        if n == 0 { break; }
                        if out_file.write_all(&buf[..n]).is_err() { break; }
                        extracted += n as u64;
                        if extracted.saturating_sub(last_report) >= REPORT_EVERY { last_report = extracted; progress_callback(extracted, total_size); }
                    }
                    #[cfg(unix)] if let Ok(mode) = entry.header().mode() { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&out_path, fs::Permissions::from_mode(mode & 0o7777)).unwrap_or(()); }
                } else {
                    if let Some(parent) = out_path.parent() { fs::create_dir_all(parent).unwrap_or(()); }
                    let _ = entry.unpack(&out_path);
                }
            }
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; for (path, mode) in dir_modes.into_iter().rev() { fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap_or(()); } }
            progress_callback(extracted, total_size);
        }
        "7z" => {
            let uni_reader = match zesven::volume::UnifiedReader::open(&archive_path) { Ok(r) => r, Err(_) => return };
            let mut streaming_arc = match zesven::StreamingArchive::open_with_config(uni_reader, "", zesven::StreamingConfig::high_performance()) { Ok(a) => a, Err(_) => return };
            let is_solid = streaming_arc.is_solid();
            let dest_path = Path::new(&dest);
            let mut top_prefix: Option<PathBuf> = None;
            let mut prefix_found = !strip_head_path;
            let mut total_size: u64 = 0;
            let mut path_map: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
            let mut solid_entries: Vec<(usize, String, u64)> = Vec::new();
            let mut work_items: Vec<(usize, PathBuf, u64)> = Vec::new();
            #[cfg(unix)] let mut file_modes: Vec<(PathBuf, u32)> = Vec::new();
            #[cfg(windows)] let mut file_win_attrs: Vec<(PathBuf, u32)> = Vec::new();
            #[cfg(unix)] let mut dir_modes: Vec<(PathBuf, u32)> = Vec::new();
            #[cfg(windows)] let mut dir_win_attrs: Vec<(PathBuf, u32)> = Vec::new();

            for (idx, e) in streaming_arc.entries_list().iter().enumerate() {
                total_size += e.size;
                let attributes = e.attributes;
                #[cfg(unix)] let unix_mode = attributes.and_then(|a| if a & 0x8000_0000 != 0 { let m = (a >> 16) & 0x7FFF; if m != 0 { Some(m) } else { None } } else { None });
                #[cfg(windows)] let win_attrs = attributes.and_then(|a| { let w = if a & 0x8000_0000 != 0 { a & 0xFFFF } else { a }; if w != 0 { Some(w) } else { None } });
                let safe_name = PathBuf::from(e.path.as_str());
                if !prefix_found { if let Some(first) = safe_name.components().next() { top_prefix = Some(PathBuf::from(first.as_os_str())); } prefix_found = true; }
                let out_path = if let Some(ref top) = top_prefix { match safe_name.strip_prefix(top) { Ok(rel) if !rel.as_os_str().is_empty() => dest_path.join(rel), _ => continue, } } else { dest_path.join(&safe_name) };
                if e.is_directory {
                    fs::create_dir_all(&out_path).unwrap_or(());
                    #[cfg(unix)] if let Some(mode) = unix_mode { dir_modes.push((out_path, mode & 0o7777)); }
                    #[cfg(windows)] if let Some(attrs) = win_attrs { dir_win_attrs.push((out_path, attrs)); }
                } else {
                    if let Some(parent) = out_path.parent() { fs::create_dir_all(parent).unwrap_or(()); }
                    if is_solid { path_map.insert(e.path.as_str().to_string(), out_path.clone()); solid_entries.push((idx, e.path.as_str().to_string(), e.size)); }
                    else { work_items.push((idx, out_path.clone(), e.size)); }
                    #[cfg(unix)] if let Some(mode) = unix_mode { file_modes.push((out_path, mode & 0o7777)); }
                    #[cfg(windows)] if let Some(attrs) = win_attrs { file_win_attrs.push((out_path, attrs)); }
                }
            }

            let extracted = Arc::new(AtomicU64::new(0));
            let last_report = Arc::new(AtomicU64::new(0));
            const REPORT_EVERY: u64 = 4 * 1024 * 1024;
            let cb = Arc::new(Mutex::new(progress_callback));
            // Multi folder (block) 7z solid archives currently fail to extract properly after folder 0 is completed... fix being debugged
            if is_solid {
                struct PSink<G> { inner: io::BufWriter<fs::File>, extracted: Arc<AtomicU64>, last_report: Arc<AtomicU64>, total_size: u64, cb: Arc<Mutex<G>> }
                impl<G: Fn(u64, u64)> Write for PSink<G> {
                    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                        let n = self.inner.write(buf)?;
                        if n > 0 {
                            let done = self.extracted.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
                            let prev = self.last_report.load(Ordering::Relaxed);
                            if done.saturating_sub(prev) >= REPORT_EVERY && self.last_report.compare_exchange(prev, done, Ordering::Relaxed, Ordering::Relaxed).is_ok() { self.cb.lock().unwrap()(done, self.total_size); }
                        }
                        Ok(n)
                    }
                    fn flush(&mut self) -> io::Result<()> { self.inner.flush() }
                }
                let _ = streaming_arc.extract_all_to_sinks(|entry| {
                    let out_path = path_map.get(entry.path.as_str())?;
                    let file = fs::File::create(out_path).ok()?;
                    if entry.size > 0 { file.set_len(entry.size).unwrap_or(()); }
                    Some(PSink {
                        inner: io::BufWriter::with_capacity(8 * 1024 * 1024, file),
                        extracted: Arc::clone(&extracted),
                        last_report: Arc::clone(&last_report),
                        total_size,
                        cb: Arc::clone(&cb),
                    })
                });
            } else {
                drop(streaming_arc);
                work_items.sort_unstable_by_key(|(_, _, sz)| *sz);
                let work = Arc::new(Mutex::new(work_items));
                let archive_path = Arc::new(archive_path);
                let num_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(10);
                let mut handles = Vec::with_capacity(num_threads);
                for _ in 0..num_threads {
                    let work = Arc::clone(&work);
                    let extracted = Arc::clone(&extracted);
                    let last_report = Arc::clone(&last_report);
                    let cb = Arc::clone(&cb);
                    let ap = Arc::clone(&archive_path);
                    handles.push(std::thread::spawn(move || {
                        let reader = match zesven::volume::UnifiedReader::open(ap.as_str()) { Ok(r) => r, Err(_) => return };
                        let mut archive = match zesven::Archive::open(reader) { Ok(a) => a, Err(_) => return };
                        loop {
                            let item = work.lock().unwrap().pop();
                            let (idx, out_path, file_size) = match item { Some(v) => v, None => break };
                            let data = match archive.extract_entry_to_vec_by_index(idx) {
                                Ok(d) => d,
                                Err(_) => { extracted.fetch_add(file_size, Ordering::Relaxed); continue; }
                            };
                            let mut out_file = match fs::File::create(&out_path) {
                                Ok(f) => f,
                                Err(_) => { extracted.fetch_add(file_size, Ordering::Relaxed); continue; }
                            };
                            if file_size > 0 { out_file.set_len(file_size).unwrap_or(()); }
                            let written = data.len() as u64;
                            out_file.write_all(&data).unwrap_or(());
                            let done = extracted.fetch_add(written, Ordering::Relaxed) + written;
                            let prev = last_report.load(Ordering::Relaxed);
                            if done.saturating_sub(prev) >= REPORT_EVERY && last_report.compare_exchange(prev, done, Ordering::Relaxed, Ordering::Relaxed).is_ok() { cb.lock().unwrap()(done, total_size); }
                        }
                    }));
                }
                for h in handles { h.join().unwrap_or(()); }
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                for (path, mode) in file_modes { fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap_or(()); }
                for (path, mode) in dir_modes.into_iter().rev() { fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap_or(()); }
            }
            #[cfg(windows)]
            {
                const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
                for (path, attrs) in file_win_attrs { if let Ok(meta) = fs::metadata(&path) { let mut perms = meta.permissions(); perms.set_readonly((attrs & FILE_ATTRIBUTE_READONLY) != 0); fs::set_permissions(&path, perms).unwrap_or(()); } }
                for (path, attrs) in dir_win_attrs.into_iter().rev() { if let Ok(meta) = fs::metadata(&path) { let mut perms = meta.permissions(); perms.set_readonly((attrs & FILE_ATTRIBUTE_READONLY) != 0); fs::set_permissions(&path, perms).unwrap_or(()); } }
            }
            cb.lock().unwrap()(extracted.load(Ordering::Relaxed), total_size);
        }
        &_ => {}
    }
}

pub(crate) fn get_full_extension(path: &str) -> Option<&str> {
    const MULTI_PART_EXTS: [&str; 2] = ["tar.gz", "tar.xz"];
    let file = path.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(path);
    for ext in MULTI_PART_EXTS {
        if file.ends_with(ext) { return Some(ext); }
    }
    file.rsplit('.').nth(1).map(|_| file.rsplitn(2, '.').collect::<Vec<_>>()[0])
}

pub fn url_safe_token(len: usize) -> String {
    let mut hasher = std::hash::RandomState::new().build_hasher();
    hasher.write_u64(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64);
    let mut seed = hasher.finish();
    let chars: String = (0..len).map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let idx = (seed % 64) as usize;
            match idx {
                0..=25 => b'A' + idx as u8,
                26..=51 => b'a' + (idx - 26) as u8,
                52..=61 => b'0' + (idx - 52) as u8,
                62 => b'-',
                _ => b'_',
            }.to_string()
        }).collect();
    chars
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KuroIndex {
    pub resource: Vec<KuroResource>,
    pub delete_files: Option<Vec<String>>,
    #[serde(rename = "patchInfos", default)]
    pub patch_infos: Option<Vec<KuroPatchEntry>>,
    #[serde(rename = "zipInfos", default)]
    pub zip_infos: Option<Vec<KuroPatchEntry>>,
    #[serde(rename = "groupResource", default)]
    pub group_resource: Option<Vec<KuroResource>>,
    #[serde(rename = "groupInfos", default)]
    pub group_infos: Option<Vec<KuroGroupInfos>>,
    #[serde(rename = "applyTypes", default)]
    pub apply_types: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KuroPatchEntry {
    pub dest: String,
    pub entries: Vec<KuroResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KuroResource {
    pub dest: String,
    pub md5: String,
    pub sample_hash: Option<String>,
    pub size: u64,
    pub from_folder: Option<String>,
    #[serde(rename = "chunkInfos", default)]
    pub chunk_infos: Option<Vec<KuroChunkInfos>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KuroChunkInfos {
    pub start: u64,
    pub end: u64,
    pub md5: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KuroGroupInfos {
    pub dest: String,
    #[serde(rename = "srcFiles", default)]
    pub src_files: Vec<KuroResource>,
    #[serde(rename = "dstFiles", default)]
    pub dst_files: Vec<KuroResource>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
pub struct TTLManifest {
    pub retcode: i32,
    pub message: String,
    pub data: Option<ManifestFile>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
pub struct ManifestFile {
    pub packages: Vec<ManifestPackage>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
pub struct ManifestPackage {
    pub git_url: String,
    pub version: String,
    pub zip_sha256: String,
    pub size: u64,
    pub package_name: String,
    pub raw_url: String,
    pub default_download_mode: String,
    pub file_list: Vec<String>,
}