//! Alcyone - 动态 DNS 客户端主程序

use alcyone::{
    config::{load_config, DEFAULT_CLOUDFLARE_TTL, DEFAULT_ALIYUN_TTL},
    cache::{read_last_ip, write_last_ip, read_zone_id_cache, update_zone_id_cache},
    ip::{get_from_interface, get_from_apis, select_best},
    dns::{CloudflareProvider, AliyunProvider, DNSProvider},
    utils::logger as log,
};
use clap::{Parser, Subcommand};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::collections::HashMap;

/// 版本信息
const APP_VERSION: &str = env!("APP_VERSION");
const LONG_VERSION: &str = concat!(
    "v", env!("APP_VERSION"),
    "\nCommit: ", env!("APP_COMMIT"),
    "\nBuild Date: ", env!("APP_BUILD_DATE")
);

/// 强大的动态 DNS 客户端
#[derive(Parser, Debug)]
#[command(name = "alcyone")]
#[command(version = APP_VERSION)]
#[command(long_version = LONG_VERSION)]
#[command(about = "强大的动态 DNS 客户端 - 支持多域名多服务商", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 运行 DDNS 更新
    Run {
        /// 配置文件路径
        #[arg(short = 'c', long = "config", default_value = "")]
        config: String,

        /// 工作目录
        #[arg(short = 'd', long = "dir", default_value = "")]
        dir: String,

        /// 忽略缓存
        #[arg(short = 'i', long = "ignore-cache", default_value = "false")]
        ignore_cache: bool,

        /// 超时时间（秒）
        #[arg(short = 't', long = "timeout", default_value_t = 300)]
        timeout: u64,
    },
}

/// 获取当前 IP
async fn get_current_ip(cfg: &alcyone::Config) -> anyhow::Result<String> {
    let infos_result = if cfg.general.get_ip.interface.is_empty() {
        get_from_apis(&cfg.general.get_ip.urls).await
    } else {
        get_from_interface(&cfg.general.get_ip.interface).await
    };

    let infos = match infos_result {
        Ok(infos) => infos,
        Err(e) => {
            log::info(&format!("Primary IP source failed: {}. Trying API fallback...", e));
            get_from_apis(&cfg.general.get_ip.urls).await?
        }
    };

    select_best(&infos)
}

/// 更新单个记录
async fn update_single_record(
    cfg: &alcyone::Config,
    record: &alcyone::RecordConfig,
    current_ip: &str,
    zone_id_cache_file: &Path,
) -> bool {
    let record_name = format!("{}.{}", record.record, record.zone);
    log::info(&format!("Processing record: {} ({})", record_name, record.provider));

    let result: anyhow::Result<()> = match record.provider.as_str() {
        "cloudflare" => {
            update_cloudflare_record(cfg, record, current_ip, zone_id_cache_file).await
        }
        "aliyun" => {
            update_aliyun_record(cfg, record, current_ip).await
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", record.provider)),
    };

    match result {
        Ok(()) => {
            log::success(&format!("Record {} updated", record_name));
            true
        }
        Err(e) => {
            log::error(&format!("Failed to update {}: {}", record_name, e));
            false
        }
    }
}

async fn update_cloudflare_record(
    cfg: &alcyone::Config,
    record: &alcyone::RecordConfig,
    current_ip: &str,
    zone_id_cache_file: &Path,
) -> anyhow::Result<()> {
    let Some(ref cf_config) = record.cloudflare else {
        anyhow::bail!("Missing cloudflare config");
    };

    let proxy_url = if record.use_proxy && !cfg.general.proxy.is_empty() {
        Some(cfg.general.proxy.clone())
    } else {
        None
    };

    let provider = CloudflareProvider::new(cf_config.api_token.clone(), proxy_url)?;

    let mut zone_id = cf_config.zone_id.clone();

    if zone_id.is_empty() {
        let cached = read_zone_id_cache(zone_id_cache_file);
        if let Some(cached_id) = cached.get(&record.zone) {
            zone_id = cached_id.clone();
        }
    }

    if zone_id.is_empty() {
        zone_id = provider.get_zone_id(&record.zone).await?;
        log::info(&format!("Zone ID fetched: {}", zone_id));
        let _ = update_zone_id_cache(zone_id_cache_file, &record.zone, &zone_id);
    }

    let ttl = if record.ttl > 0 { record.ttl } else { cf_config.ttl.max(DEFAULT_CLOUDFLARE_TTL) };
    let mut extra = HashMap::new();
    if record.proxied || cf_config.proxied {
        extra.insert("proxied".to_string(), "true".to_string());
    }

    provider.upsert_record(&record.zone, &record.record, current_ip, ttl, &extra).await
}

async fn update_aliyun_record(
    _cfg: &alcyone::Config,
    record: &alcyone::RecordConfig,
    current_ip: &str,
) -> anyhow::Result<()> {
    let Some(ref aliyun_config) = record.aliyun else {
        anyhow::bail!("Missing aliyun config");
    };

    if record.use_proxy {
        log::warning("Aliyun provider does not support proxy");
    }

    let provider = AliyunProvider::new(
        aliyun_config.access_key_id.clone(),
        aliyun_config.access_key_secret.clone(),
    );

    let ttl = if record.ttl > 0 { record.ttl } else { aliyun_config.ttl.max(DEFAULT_ALIYUN_TTL) };

    provider.upsert_record(&record.zone, &record.record, current_ip, ttl, &HashMap::new()).await
}

/// 设置信号处理器
fn setup_signal_handler() -> Arc<AtomicBool> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    tokio::spawn(async move {
        while let Ok(()) = tokio::signal::ctrl_c().await {
            shutdown_clone.store(true, Ordering::SeqCst);
            log::warning("Shutdown requested");
        }
    });

    shutdown
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { config, dir, ignore_cache, timeout: _ } => {
            // 解析配置路径
            let config_path = if config.is_empty() {
                if dir.is_empty() {
                    eprintln!("错误：缺少配置文件参数 --config/-c");
                    std::process::exit(1);
                }
                let default_config = Path::new(&dir).join("config.json");
                if !default_config.exists() {
                    eprintln!("配置文件未找到：{}", default_config.display());
                    std::process::exit(1);
                }
                default_config
            } else {
                Path::new(&config).to_path_buf()
            };

            // 确定工作目录
            let work_dir = if dir.is_empty() {
                config_path.parent().unwrap_or(Path::new(".")).to_path_buf()
            } else {
                Path::new(&dir).to_path_buf()
            };

            // 加载配置
            let cfg = match load_config(&config_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to load config: {}", e);
                    std::process::exit(1);
                }
            };

            // 初始化日志（固定输出到 stdout，由 systemd/cron 管理）
            if let Err(e) = log::init("shell") {
                eprintln!("Failed to initialize logger: {}", e);
                std::process::exit(1);
            }

            // 设置信号处理器
            let _shutdown = setup_signal_handler();

            log::info(&format!("alcyone starting with {} record(s)", cfg.records.len()));

            // 获取当前 IP
            let current_ip = match get_current_ip(&cfg).await {
                Ok(ip) => ip,
                Err(e) => {
                    log::error(&format!("Failed to get current IP: {}", e));
                    std::process::exit(1);
                }
            };
            log::info(&format!("Current IPv6 address: {}", current_ip));

            // 检查缓存
            let cache_file = work_dir.join("cache.lastip");
            let last_ip = read_last_ip(&cache_file);
            if !ignore_cache && !last_ip.is_empty() {
                if last_ip == current_ip {
                    log::info(&format!("IP has not changed: {}", current_ip));
                } else {
                    log::info(&format!("IP changed from {} to {}", last_ip, current_ip));
                }
            }

            // Zone ID 缓存
            let zone_id_cache_file = work_dir.join("cache.zoneid.json");

            // 并发更新所有记录
            let mut handles = Vec::new();
            let cfg = Arc::new(cfg);
            for record in cfg.records.iter() {
                let cfg_ref = Arc::clone(&cfg);
                let record = record.clone();
                let current_ip = current_ip.clone();
                let zone_id_cache = zone_id_cache_file.clone();

                let handle = tokio::spawn(async move {
                    update_single_record(&cfg_ref, &record, &current_ip, &zone_id_cache).await
                });

                handles.push(handle);
            }

            // 等待所有任务完成
            let mut success_count = 0;
            let mut fail_count = 0;

            for handle in handles {
                if let Ok(success) = handle.await {
                    if success {
                        success_count += 1;
                    } else {
                        fail_count += 1;
                    }
                }
            }

            log::info(&format!("Update completed: {} succeeded, {} failed", success_count, fail_count));

            // 更新缓存
            if success_count > 0 && last_ip != current_ip {
                if let Err(e) = write_last_ip(&cache_file, &current_ip) {
                    log::warning(&format!("Warning: failed to write cache: {}", e));
                }
            }

            if fail_count > 0 {
                std::process::exit(1);
            }
        }
    }
}
