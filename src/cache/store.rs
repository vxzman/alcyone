//! 缓存存储

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// 读取上次缓存的 IP
pub fn read_last_ip<P: AsRef<Path>>(path: P) -> String {
    fs::read_to_string(path.as_ref())
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// 写入 IP 到缓存文件
pub fn write_last_ip<P: AsRef<Path>>(path: P, ip: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path.as_ref())?;

    file.write_all(ip.as_bytes())?;
    file.sync_all()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path.as_ref(), fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// 读取 Zone ID 缓存
pub fn read_zone_id_cache<P: AsRef<Path>>(path: P) -> HashMap<String, String> {
    fs::read_to_string(path.as_ref())
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// 更新 Zone ID 缓存
pub fn update_zone_id_cache<P: AsRef<Path>>(
    path: P,
    zone: &str,
    zone_id: &str,
) -> anyhow::Result<()> {
    let mut cache: HashMap<String, String> = read_zone_id_cache(&path);
    cache.insert(zone.to_string(), zone_id.to_string());
    let content = serde_json::to_string_pretty(&cache)?;
    fs::write(path, content)?;
    Ok(())
}
