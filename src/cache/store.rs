//! 缓存存储
//!
//! IP 缓存文件格式（每行一条记录）：
//! ```text
//! 2026-05-04T12:00:00  2409:8a6c:1c35:53c0:ccd4:7618:4693:c9e1
//! 2026-05-03T08:30:00  2409:8a6c:1c35:53c0:2d7a:5131:630c:e582
//! ```

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use chrono::Local;

const MAX_IP_HISTORY: usize = 10;

/// 读取最新缓存的 IP（返回最新一行）
pub fn read_last_ip<P: AsRef<Path>>(path: P) -> String {
    fs::read_to_string(path.as_ref())
        .ok()
        .and_then(|content| {
            content
                .lines()
                .rev()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        Some(parts[1].to_string())
                    } else {
                        None
                    }
                })
                .next()
        })
        .unwrap_or_default()
}

/// 追加 IP 到缓存文件（带时间戳）
pub fn write_last_ip<P: AsRef<Path>>(path: P, ip: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }

    let now = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let new_line = format!("{}  {}\n", now, ip);

    // 读取现有内容
    let existing = fs::read_to_string(path.as_ref()).ok().unwrap_or_default();
    let lines: Vec<&str> = existing.lines().collect();

    // 追加新行，限制最大历史记录数
    let mut all_lines = lines;
    all_lines.push(new_line.trim());
    if all_lines.len() > MAX_IP_HISTORY {
        all_lines = all_lines[all_lines.len() - MAX_IP_HISTORY..].to_vec();
    }

    let content = all_lines.join("\n") + "\n";

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path.as_ref())?;

    file.write_all(content.as_bytes())?;
    file.sync_all()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path.as_ref(), fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// 获取所有 IP 历史记录
pub fn read_ip_history<P: AsRef<Path>>(path: P) -> Vec<(String, String)> {
    fs::read_to_string(path.as_ref())
        .ok()
        .map(|content| {
            content
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        Some((parts[0].to_string(), parts[1].to_string()))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
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
