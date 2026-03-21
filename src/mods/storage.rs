use rustix::fs::statvfs;
use serde_json::{json, Value};
use std::fs;

const EXCLUDED_FS: &[&str] = &[
    "tmpfs", "devtmpfs", "squashfs", "efivarfs", "overlay",
    "proc", "sysfs", "devpts", "securityfs", "cgroup", "cgroup2",
    "pstore", "debugfs", "hugetlbfs", "mqueue", "configfs",
    "fusectl", "tracefs", "bpf", "nsfs", "ramfs", "binfmt_misc",
    "autofs", "rpc_pipefs", "fuse.portal",
];

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let mounts = match fs::read_to_string("/proc/mounts") {
        Ok(s) => s,
        Err(_) => return json!({"disks": []}),
    };

    let mut disks = Vec::new();

    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let fstype = parts[2];
        let mount = parts[1];

        if EXCLUDED_FS.iter().any(|&e| fstype == e) {
            continue;
        }

        if let Ok(stat) = statvfs(mount) {
            let bsize = stat.f_frsize as u64;
            let size = stat.f_blocks * bsize;
            let avail = stat.f_bavail * bsize;
            let free = stat.f_bfree * bsize;
            let used = size.saturating_sub(free);
            let percent = if size > 0 { used * 100 / size } else { 0 };

            disks.push(json!({
                "mount": mount,
                "size_bytes": size,
                "used_bytes": used,
                "avail_bytes": avail,
                "percent": percent,
            }));
        }
    }

    json!({ "disks": disks })
}
