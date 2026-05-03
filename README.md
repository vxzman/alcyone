# Alcyone — 动态 DNS 客户端 (Rust) v1

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20FreeBSD-yellow)](README.md)
[![Rust](https://img.shields.io/badge/Rust-1.75+-orange)](README.md)

> **Alcyone** 名称源自 Aiolos 的变体，象征快速与灵动。
> 本项目寓意**快速响应网络变化，灵动更新 DNS 记录**。

**alcyone** 是一个用 Rust 编写的轻量级动态 DNS (DDNS) 客户端，支持多域名、多服务商、IPv6，具备跨平台能力和丰富的日志输出。

---

## 📑 目录

- [核心特性](#-核心特性)
- [快速开始](#-快速开始)
- [配置详解](#-配置详解)
- [命令行参数](#-命令行参数)
- [部署](#-部署)
- [故障排查](#-故障排查)
- [贡献与许可](#-贡献与许可)

---

## ✨ 核心特性

- **多平台**：Linux、FreeBSD；macOS/OpenBSD 默认降级为 URL 方式
- **多服务商**：Cloudflare、阿里云 DNS
- **并发更新**：同时更新多条 DNS 记录
- **IPv6 优先**：原生支持 IPv6 地址动态获取与更新
- **变量引用**：敏感信息集中管理，通过 `$变量名` 引用
- **缓存机制**：IP 未变化时不触发 API 更新
- **固定 stdout 日志**：由 systemd/cron 统一管理

---

## 🚀 快速开始

### 1. 安装依赖

```bash
# Ubuntu / Debian
sudo apt install build-essential pkg-config libssl-dev

# FreeBSD
pkg install gcc openssl
```

### 2. 构建

```bash
cargo build --release
# 或使用构建脚本
./build.sh v1.0.0
```

### 3. 配置

```bash
cp config.example.json config.json
# 编辑 config.json，在 environment 中填入凭证
```

### 4. 运行

```bash
./target/release/alcyone run -c config.json
```

---

## 📝 配置详解

配置文件采用 JSON 格式，结构如下：

```jsonc
{
    // 环境变量和敏感值集中存放，可在配置中引用
    "environment": {
        "cloudflare_token": "your_cloudflare_api_token_here",
        "cloudflare_zone_id": "your_cloudflare_zone_id_here",
        "aliyun_access_key_id": "your_aliyun_access_key_id_here",
        "aliyun_access_key_secret": "your_aliyun_access_key_secret_here"
    },
    "general": {
        // IP 获取方式：interface（网卡，仅 Linux/FreeBSD）或 urls（API），二选一
        "get_ip": {
            "interface": "eth0",
            "urls": [
                "https://ipv6.icanhazip.com",
                "https://6.ipw.cn",
                "https://v6.ipv6-test.com/api/myip.php"
            ]
        },
        // 全局代理（仅 Cloudflare 支持）
        "proxy": ""
    },
    "records": [
        {
            "provider": "cloudflare",        // 服务商：cloudflare 或 aliyun
            "zone": "example.com",           // 主域名
            "record": "dev",                 // 子域名（@ 表示根域名）
            "ttl": 300,                      // TTL（秒），可选
            "proxied": false,                // Cloudflare CDN 代理，仅 Cloudflare
            "use_proxy": false,              // 是否使用全局 proxy，仅 Cloudflare
            "cloudflare": {                  // provider=cloudflare 时必需
                "api_token": "$cloudflare_token",
                "zone_id": "$cloudflare_zone_id"
            }
        },
        {
            "provider": "aliyun",
            "zone": "example.cn",
            "record": "www",
            "ttl": 600,
            "use_proxy": false,              // 阿里云不支持代理
            "aliyun": {                      // provider=aliyun 时必需
                "access_key_id": "$aliyun_access_key_id",
                "access_key_secret": "$aliyun_access_key_secret"
            }
        }
    ]
}
```

### 变量引用

使用 `$变量名` 引用 `environment` 中定义的值：

```jsonc
{
    "environment": { "my_token": "abc123" },
    "records": [{
        "cloudflare": { "api_token": "$my_token" }
    }]
}
```

> ⚠️ 仅支持引用 `environment` 中定义的变量，不支持系统环境变量或默认值。

### 服务商对比

| 字段 | Cloudflare | 阿里云 DNS |
|------|------------|------------|
| 认证字段 | `api_token`、`zone_id` | `access_key_id`、`access_key_secret` |
| 代理支持 | ✅ HTTP/SOCKS5 | ❌ 不支持 |
| CDN 代理 | `proxied` 字段 | 不支持 |
| 最小 TTL | 120 秒 | 1 秒 |

### 顶层字段

| 字段 | 必需 | 描述 |
|------|------|------|
| `environment` | 否 | 敏感值集中存放，可在配置中引用 |
| `general` | 是 | 全局配置（IP 获取、代理） |
| `records` | 是 | DNS 记录列表（至少一条） |

---

## 🖥️ 命令行参数

```bash
alcyone run [options]
```

| 参数 | 简写 | 默认值 | 描述 |
|------|------|--------|------|
| `--config` | `-c` | 无 | 配置文件路径 |
| `--dir` | `-d` | 无 | 工作目录（存放缓存） |
| `--ignore-cache` | `-i` | `false` | 忽略缓存，强制更新 |
| `--timeout` | `-t` | `300` | 超时时间（秒） |

**用法示例**：

```bash
# 指定配置
alcyone run -c /etc/alcyone/config.json
# 强制更新
alcyone run -c config.json -i
# 自定义超时时间
alcyone run -c config.json -t 600
```

---

## 📦 部署

### systemd 部署

创建 `/etc/systemd/system/alcyone.service`：

```ini
[Unit]
Description=Alcyone DDNS Client
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/alcyone run -c /etc/alcyone/config.json -d /etc/alcyone
WorkingDirectory=/etc/alcyone

[Install]
WantedBy=multi-user.target
```

创建定时器 `/etc/systemd/system/alcyone.timer`：

```ini
[Unit]
Description=Run Alcyone DDNS every 10 minutes

[Timer]
OnBootSec=5min
OnUnitActiveSec=10min
Unit=alcyone.service

[Install]
WantedBy=timers.target
```

启用：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now alcyone.timer
```

### Crontab 部署

```bash
# 每 10 分钟执行一次
*/10 * * * * /usr/local/bin/alcyone run -c /etc/alcyone/config.json -d /etc/alcyone
```

---

## 🐛 故障排查

| 错误信息 | 解决方案 |
|----------|----------|
| `Environment variable 'xxx' not found` | 检查 `environment` 中是否定义了对应变量 |
| `Failed to get current IP` | 检查网卡名称（`ip addr`）；确保启用 IPv6；尝试用 urls 方式 |
| `Cloudflare upsert failed: Invalid API Token` | 检查 Token 及 `Zone:DNS:Edit` 权限 |

---

## 贡献与许可

- 提交 Issue / PR 欢迎贡献
- 采用 **MIT License**

**Made with ❤️ by the Alcyone Team**
