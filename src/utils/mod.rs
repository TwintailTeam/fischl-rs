use std::{fs, io};
use std::fs::File;
use std::path::{Path};
use compress_tools::Ownership;
use reqwest::header::USER_AGENT;
use crate::utils::codeberg_structs::CodebergRelease;
use crate::utils::github_structs::GithubRelease;

pub(crate) mod github_structs;
pub(crate) mod codeberg_structs;
mod free_space;
pub mod game;

pub fn get_github_release(repository: String) -> Option<GithubRelease> {
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

pub fn get_codeberg_release(repository: String) -> Option<CodebergRelease> {
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

pub fn get_tukanrepo_release(repository: String) -> Option<CodebergRelease> {
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

pub fn extract_archive(archive_path: String, extract_dest: String, move_subdirs: bool) -> Option<bool> {
    let src = Path::new(&archive_path);
    let dest = Path::new(&extract_dest);

    if !src.exists() {
        None
    } else if !dest.exists() {
        fs::create_dir_all(&dest).unwrap();
        let mut file = File::open(&src).unwrap();
        compress_tools::uncompress_archive(&mut file, &dest, Ownership::Preserve).unwrap();
        fs::remove_file(&src).unwrap();

        if move_subdirs {
            copy_dir_all(&dest).unwrap();
        }

        Some(true)
    } else {
        let mut file = File::open(&src).unwrap();
        compress_tools::uncompress_archive(&mut file, &dest, Ownership::Preserve).unwrap();
        fs::remove_file(&src).unwrap();

        if move_subdirs {
            copy_dir_all(&dest).unwrap();
        }

        Some(true)
    }
}

pub fn copy_dir_all(dst: impl AsRef<Path>) -> io::Result<()> {
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

/*#[derive(Debug, Clone)]
pub struct GameManifest {
    pub version: i32,
    pub display_name: String,
    pub biz: String,
    pub latest_version: String,
    pub game_versions: Vec<GameVersion>,
    pub telemetry_hosts: Vec<String>,
    pub paths: GamePaths,
    pub assets: VersionAssets,
    pub extra: GameExtras
}

#[derive(Debug, Clone)]
pub struct GameVersion {
    pub metadata: VersionMetadata,
    pub assets: VersionAssets,
    pub game: VersionGameFiles,
    pub audio: VersionAudioFiles
}

#[derive(Debug, Clone)]
pub struct GamePaths {
    pub exe_filename: String,
    pub installation_dir: String,
    pub screenshot_dir: String,
    pub screenshot_dir_relative_to: String
}

#[derive(Debug, Clone)]
pub struct VersionMetadata {
    pub versioned_name: String,
    pub version: String,
    pub game_hash: String
}

#[derive(Debug, Clone)]
pub struct VersionAssets {
    pub game_icon: String,
    pub game_background: String
}

#[derive(Debug, Clone)]
pub struct VersionGameFiles {
    pub full: Vec<FullGameFile>,
    pub diff: Vec<DiffGameFile>
}

#[derive(Debug, Clone)]
pub struct FullGameFile {
    pub file_url: String,
    pub compressed_size: String,
    pub decompressed_size: String,
    pub file_hash: String,
    pub file_path: String
}

#[derive(Debug, Clone)]
pub struct DiffGameFile {
    pub file_url: String,
    pub compressed_size: String,
    pub decompressed_size: String,
    pub file_hash: String,
    pub diff_type: String,
    pub original_version: String,
    pub delete_files: Vec<String>
}

#[derive(Debug, Clone)]
pub struct VersionAudioFiles {
    pub full: Vec<FullAudioFile>,
    pub diff: Vec<DiffAudioFile>
}

#[derive(Debug, Clone)]
pub struct FullAudioFile {
    pub file_url: String,
    pub compressed_size: String,
    pub decompressed_size: String,
    pub file_hash: String,
    pub language: String
}

#[derive(Debug, Clone)]
pub struct DiffAudioFile {
    pub file_url: String,
    pub compressed_size: String,
    pub decompressed_size: String,
    pub file_hash: String,
    pub diff_type: String,
    pub original_version: String,
    pub language: String
}

#[derive(Debug, Clone)]
pub struct GamePreload {
    pub metadata: Option<VersionMetadata>,
    pub game: Option<VersionGameFiles>,
    pub audio: Option<VersionAudioFiles>
}

#[derive(Debug, Clone)]
pub struct GameTweakSwitches {
    pub fps_unlocker: bool,
    pub jadeite: bool,
    pub xxmi: bool
}

#[derive(Debug, Clone)]
pub struct GameExtras {
    pub preload: Option<GamePreload>,
    pub switches: GameTweakSwitches,
    pub fps_unlock_options: Vec<String>,
}*/