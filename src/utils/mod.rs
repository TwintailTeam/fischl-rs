use std::{fs, io};
use std::io::{Error, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use compress_tools::Ownership;
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use crate::utils::codeberg_structs::CodebergRelease;
use crate::utils::github_structs::GithubRelease;

pub(crate) mod github_structs;
pub(crate) mod codeberg_structs;
pub(crate) mod proto;
pub mod free_space;
pub mod game;
pub mod downloader;

pub(crate) fn get_github_release(repository: String) -> Option<GithubRelease> {
    if repository.is_empty() {
        None
    } else {
        let url = format!("https://api.github.com/repos/{}/releases/latest", repository);
        let client = reqwest::blocking::Client::new();
        let response = client.get(url).header(USER_AGENT, "lib/fischl-rs").send();
        if response.is_ok() {
            let list = response.unwrap();
            let jsonified: GithubRelease = list.json().unwrap();
            Some(jsonified)
        } else {
            None
        }
    }
}

pub(crate) fn get_codeberg_release(repository: String) -> Option<CodebergRelease> {
    if repository.is_empty() {
        None
    } else {
        let url = format!("https://codeberg.org/api/v1/repos/{}/releases?draft=false&pre-release=false", repository);
        let client = reqwest::blocking::Client::new();
        let response = client.get(url).header(USER_AGENT, "lib/fischl-rs").send();
        if response.is_ok() {
            let list = response.unwrap();
            let jsonified: CodebergRelease = list.json().unwrap();
            Some(jsonified)
        } else {
            None
        }
    }
}

pub(crate) fn get_tukanrepo_release(repository: String) -> Option<CodebergRelease> {
    if repository.is_empty() {
        None
    } else {
        let url = format!("https://repo.tukandev.com/api/v1/repos/{}/releases?draft=false&pre-release=false", repository);
        let client = reqwest::blocking::Client::new();
        let response = client.get(url).header(USER_AGENT, "lib/fischl-rs").send();
        if response.is_ok() {
            let list = response.unwrap();
            let jsonified: CodebergRelease = list.json().unwrap();
            Some(jsonified)
        } else {
            None
        }
    }
}

pub fn extract_archive(archive_path: String, extract_dest: String, move_subdirs: bool) -> bool {
    let src = Path::new(&archive_path);
    let dest = Path::new(&extract_dest);

    if !src.exists() {
        false
    } else if !dest.exists() {
        fs::create_dir_all(&dest).unwrap();
        let mut file = fs::File::open(&src).unwrap();
        compress_tools::uncompress_archive(&mut file, &dest, Ownership::Preserve).unwrap();
        fs::remove_file(&src).unwrap();
        if move_subdirs {
            copy_dir_all(&dest).unwrap();
        }
        true
    } else {
        let mut file = fs::File::open(&src).unwrap();
        compress_tools::uncompress_archive(&mut file, &dest, Ownership::Preserve).unwrap();
        fs::remove_file(&src).unwrap();
        if move_subdirs {
            copy_dir_all(&dest).unwrap();
        }
        true
    }
}

pub fn assemble_multipart_archive(parts: Vec<String>, dest: String) -> bool {
    let first = parts.get(0).unwrap().strip_suffix(".001").unwrap();
    let fap = Path::new(&dest).join(first);
    let mut out = match fs::File::create(&fap) {
        Ok(f) => f,
        Err(_) => return false,
    };

    for p in parts.clone() {
        let mut buf = Vec::new();
        let partp = Path::new(&dest).join(&p);

        let mut file = fs::File::open(&partp).unwrap();
        file.read_to_end(&mut buf).unwrap();
        out.write_all(&buf).unwrap();
        fs::remove_file(&partp).unwrap();
    }

    if out.flush().is_err() {
        return false;
    }

    true
}

pub(crate) fn copy_dir_all(dst: impl AsRef<Path>) -> io::Result<()> {
    for entry in fs::read_dir(dst.as_ref())? {
        let entry = entry?;
        let ty = entry.file_type()?;

        if ty.is_dir() {
            move_dir_and_files(&entry.path(), dst.as_ref())?;
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

fn move_dir_and_files(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;

        if ty.is_file() {
            fs::rename(entry.path(), dst.join(entry.file_name()))?;
        } else {
            let new_path = dst.join(entry.file_name());
            fs::rename(entry.path(), new_path)?;
        }
    }
    Ok(())
}

pub(crate) fn move_all<'a>(src: &'a Path, dst: &'a Path) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if !dst.exists() {
            tokio::fs::create_dir_all(dst).await?;
        }

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
    if bytes > 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if bytes > 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes > 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} B", bytes)
    }
}

pub fn wait_for_process<F>(process_name: &str, delay_ms: u64, retries: usize, mut callback: F) -> bool where F: FnMut(bool) -> bool {
    let mut sys = sysinfo::System::new_all();

    for _ in 0..retries {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let found = sys.processes().values().any(|process| process.name() == process_name);
        if callback(found) {
            return found;
        }
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }
    false
}

pub fn patch<T: Into<PathBuf> + std::fmt::Debug>(file: T, patch: T, output: T) -> io::Result<()> {
    // Must have it as a system installed for now
    let output = Command::new("hpatchz")
        .arg("-f")
        .arg(file.into().as_os_str())
        .arg(patch.into().as_os_str())
        .arg(output.into().as_os_str())
        .output()?;

    if String::from_utf8_lossy(output.stdout.as_slice()).contains("patch ok!") {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        Err(Error::new(ErrorKind::Other, format!("Failed to apply hdiff patch: {err}")))
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct KuroFile {
    pub url: String,
    pub path: String,
    pub hash: String,
    pub size: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KuroIndex {
    pub resource: Vec<KuroResource>
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KuroResource {
    pub dest: String,
    pub md5: String,
    pub size: u64
}