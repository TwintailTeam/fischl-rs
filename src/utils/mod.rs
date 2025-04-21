use std::{fs, io};
use std::fs::File;
use std::path::{Path};
use compress_tools::Ownership;
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use crate::utils::codeberg_structs::CodebergRelease;
use crate::utils::github_structs::GithubRelease;

pub(crate) mod github_structs;
pub(crate) mod codeberg_structs;
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
        let mut file = File::open(&src).unwrap();
        compress_tools::uncompress_archive(&mut file, &dest, Ownership::Preserve).unwrap();
        fs::remove_file(&src).unwrap();
        if move_subdirs {
            copy_dir_all(&dest).unwrap();
        }
        true
    } else {
        let mut file = File::open(&src).unwrap();
        compress_tools::uncompress_archive(&mut file, &dest, Ownership::Preserve).unwrap();
        fs::remove_file(&src).unwrap();
        if move_subdirs {
            copy_dir_all(&dest).unwrap();
        }
        true
    }
}

pub(crate) fn copy_dir_all(dst: impl AsRef<Path>) -> io::Result<()> {
    for entry in fs::read_dir(dst.as_ref())? {
        let entry = entry?;
        let ty = entry.file_type()?;

        if ty.is_dir() {
            move_dir_and_files(&entry.path(), dst.as_ref())?;
            fs::remove_dir(entry.path())?;
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

pub fn wait_for_process(process_name: &str, callback: impl FnOnce() -> bool) -> bool {
    let sys = sysinfo::System::new_all();
    let func = callback();

    for (_pid, process) in sys.processes() {
        if process.name() == process_name {
            return func;
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    false
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