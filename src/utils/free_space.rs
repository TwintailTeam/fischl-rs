use std::path::Path;
use sysinfo::Disks;

pub fn available(path: impl AsRef<Path>) -> Option<u64> {
    let mut disks = Disks::new_with_refreshed_list();
    disks.sort_by(|a, b| {
        let a = a.mount_point().as_os_str().len();
        let b = b.mount_point().as_os_str().len();
        a.cmp(&b).reverse()
    });

    let path = if cfg!(target_os = "linux") { path.as_ref().read_link().unwrap_or(path.as_ref().to_path_buf()) } else { path.as_ref().to_path_buf() };
    for disk in disks.iter() {
        let fixed = if cfg!(target_os = "linux") { disk.mount_point().read_link().unwrap_or(disk.mount_point().to_path_buf()) } else { disk.mount_point().to_path_buf() };
        println!("{} | mountpoint: {}", path.display(), fixed.display());
        if path.starts_with(fixed) { return Some(disk.available_space()); }
    }
    None
}

pub fn is_same_disk(path1: impl AsRef<Path>, path2: impl AsRef<Path>) -> bool {
    let mut disks = Disks::new_with_refreshed_list();
    disks.sort_by(|a, b| {
        let a = a.mount_point().as_os_str().len();
        let b = b.mount_point().as_os_str().len();
        a.cmp(&b).reverse()
    });

    let path1 = if cfg!(target_os = "linux") { path1.as_ref().read_link().unwrap_or(path1.as_ref().to_path_buf()) } else { path1.as_ref().to_path_buf() };
    let path2 = if cfg!(target_os = "linux") { path2.as_ref().read_link().unwrap_or(path2.as_ref().to_path_buf()) } else { path2.as_ref().to_path_buf() };
    for disk in disks.iter() {
        let disk_path = if cfg!(target_os = "linux") { disk.mount_point().read_link().unwrap_or(disk.mount_point().to_path_buf()) } else { disk.mount_point().to_path_buf() };
        if path1.starts_with(disk_path.clone()) && path2.starts_with(disk_path) { return true; }
    }
    false
}
