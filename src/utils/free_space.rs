use std::path::Path;
use sysinfo::Disks;

pub fn available(path: impl AsRef<Path>) -> Option<u64> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let mut disks = Disks::new_with_refreshed_list();
        disks.sort_by(|a, b| {
            let a = a.mount_point().as_os_str().len();
            let b = b.mount_point().as_os_str().len();
            a.cmp(&b).reverse()
        });

        let path = path.as_ref().to_owned();
        for disk in disks.iter() {
            let dp = disk.mount_point().to_path_buf();
            if path.starts_with(dp) { return Some(disk.available_space()); }
        }
        None
    }

    /*#[cfg(target_os = "linux")]
    {
        use std::os::linux::fs::MetadataExt;
        let mut disks = Disks::new_with_refreshed_list();
        disks.sort_by(|a, b| {
            let a = a.mount_point().as_os_str().len();
            let b = b.mount_point().as_os_str().len();
            a.cmp(&b).reverse()
        });

        let path = std::fs::metadata(path.as_ref()).ok()?.st_dev();
        for disk in disks.iter() {
            let dp = std::fs::metadata(&disk.mount_point()).ok()?.st_dev();
            if dp == path { return Some(disk.available_space()); }
        }
        None
    }*/
}

pub fn is_same_disk(path1: impl AsRef<Path>, path2: impl AsRef<Path>) -> bool {
    let mut disks = Disks::new_with_refreshed_list();
    disks.sort_by(|a, b| {
        let a = a.mount_point().as_os_str().len();
        let b = b.mount_point().as_os_str().len();
        a.cmp(&b).reverse()
    });

    let mut path1 = path1.as_ref().to_path_buf();
    let mut path2 = path2.as_ref().to_path_buf();
    for disk in disks.iter() {
        let mut disk_path = disk.mount_point().to_path_buf();
        if path1.starts_with(disk_path.clone()) && path2.starts_with(disk_path) { return true; }
    }
    false
}
