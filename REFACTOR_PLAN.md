# Alcyone 项目重构计划

## 当前结构分析

### 现有文件结构
```
src/
├── main.rs              # 452 行 - 耦合了 CLI、配置加载、业务逻辑
├── log.rs               # 161 行 - 日志系统
├── config.rs            # 198 行 - 配置模型 + 加载逻辑
├── cache.rs             # 37 行 - 缓存读写
├── http_client.rs       # 142 行 - HTTP 客户端
├── ip_getter.rs         # 508 行 - IP 获取（混合 Linux/BSD 实现）
└── provider/
    ├── mod.rs           # 14 行 - DNS trait
    ├── cloudflare.rs    # 281 行 - Cloudflare 实现
    └── aliyun.rs        # 222 行 - 阿里云实现
```

### 存在的问题
1. **main.rs 过大**：包含 CLI 解析、配置加载、业务逻辑、信号处理
2. **模块职责不清**：config.rs 既有模型定义又有加载逻辑
3. **平台代码混合**：ip_getter.rs 中 Linux/BSD 代码混在一起
4. **难以扩展**：添加新 DNS 提供商需要修改多处代码
5. **测试困难**：缺乏清晰的接口抽象，难以编写单元测试

---

## 目标结构

```
src/
├── main.rs              # < 100 行 - 仅 CLI 和启动
├── lib.rs               # 库入口，重新导出公共 API
│
├── bin/
│   └── alcyone.rs       # 二进制入口（可选拆分）
│
├── config/              # 配置模块
│   ├── mod.rs           # 模块导出
│   ├── model.rs         # 配置模型 (Config, RecordConfig 等)
│   ├── loader.rs        # 配置加载器
│   └── validator.rs     # 配置验证（待添加）
│
├── ip/                  # IP 获取模块
│   ├── mod.rs           # 模块导出
│   ├── model.rs         # Ipv6Info, IpSource trait
│   ├── source.rs        # IP 源管理器（支持 fallback）
│   ├── interface.rs     # 网卡实现（Linux/BSD 分离）
│   └── api.rs           # HTTP API 实现
│
├── dns/                 # DNS 提供商模块
│   ├── mod.rs           # 模块导出
│   ├── provider.rs      # DNSProvider trait
│   ├── registry.rs      # 提供商注册表
│   ├── cloudflare.rs    # Cloudflare 实现
│   ├── aliyun.rs        # 阿里云实现
│   └── (future)         # 未来可扩展：DNSPod, HuaweiCloud 等
│
├── http/                # HTTP 客户端模块
│   ├── mod.rs           # 模块导出
│   └── client.rs        # HttpClient + 重试逻辑
│
├── cache/               # 缓存模块
│   ├── mod.rs           # 模块导出
│   ├── store.rs         # IP 缓存
│   └── zone_cache.rs    # Zone ID 缓存（待添加）
│
├── core/                # 核心业务逻辑
│   ├── mod.rs           # 模块导出
│   ├── runner.rs        # DDNS 运行器
│   └── updater.rs       # DNS 更新器
│
└── utils/               # 工具模块
    ├── mod.rs           # 模块导出
    └── logger.rs        # 日志系统
```

---

## 重构步骤

### 阶段一：基础重构（1-2 天）
- [ ] 移动日志模块到 `utils/logger.rs`
- [ ] 拆分 config.rs 为 model.rs + loader.rs
- [ ] 移动 cache.rs 到 `cache/store.rs`
- [ ] 移动 http_client.rs 到 `http/client.rs`
- [ ] 创建 lib.rs，重新导出公共 API

### 阶段二：IP 获取模块重构（2-3 天）
- [ ] 定义 `IpSource` trait
- [ ] 实现 `InterfaceSource`（网卡）
- [ ] 实现 `ApiSource`（HTTP API）
- [ ] 创建 `IpSourceManager` 支持 fallback
- [ ] Linux/BSD 代码分离到不同文件

### 阶段三：DNS 提供商重构（2-3 天）
- [ ] 定义 `DNSProvider` trait
- [ ] 创建 `ProviderRegistry` 注册表
- [ ] 重构 Cloudflare 实现
- [ ] 重构阿里云实现
- [ ] 支持动态注册新提供商

### 阶段四：核心逻辑重构（2-3 天）
- [ ] 创建 `DnsUpdater` 负责单个记录更新
- [ ] 创建 `Runner` 协调整个流程
- [ ] 简化 main.rs 只负责 CLI
- [ ] 添加配置验证

### 阶段五：测试和文档（2-3 天）
- [ ] 添加单元测试
- [ ] 添加集成测试
- [ ] 生成 API 文档
- [ ] 编写重构指南

---

## 关键设计

### 1. IP 获取抽象

```rust
/// IP 源 trait
#[async_trait::async_trait]
pub trait IpSource: Send + Sync {
    fn name(&self) -> &str;
    async fn get_ipv6(&self) -> anyhow::Result<Vec<Ipv6Info>>;
}

/// IP 源管理器，支持多源 fallback
pub struct IpSourceManager {
    primary: Box<dyn IpSource>,
    fallback: Box<dyn IpSource>,
}
```

### 2. DNS 提供商注册表

```rust
/// DNS 提供商 trait
#[async_trait::async_trait]
pub trait DNSProvider: Send + Sync {
    async fn upsert_record(
        &self,
        zone: &str,
        record_name: &str,
        ip: &str,
        ttl: u32,
        extra: &HashMap<String, String>,
    ) -> anyhow::Result<()>;
}

/// 提供商注册表
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn DNSProvider>>,
}

impl ProviderRegistry {
    pub fn register<P: DNSProvider + 'static>(&mut self, name: String, provider: P);
    pub fn get(&self, name: &str) -> Option<Arc<dyn DNSProvider>>;
}
```

### 3. 配置加载器

```rust
pub struct ConfigLoader {
    base_dir: Option<PathBuf>,
}

impl ConfigLoader {
    pub fn load<P: AsRef<Path>>(&self, path: P) -> Result<Config, ConfigError>;
}

/// 配置构建器
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    pub fn new() -> Self;
    pub fn with_environment(mut self, env: HashMap<String, String>) -> Self;
    pub fn with_records(mut self, records: Vec<RecordConfig>) -> Self;
    pub fn build(self) -> Config;
}
```

---

## 收益

### 代码质量
- **可维护性**：每个模块职责清晰，易于理解和修改
- **可测试性**：接口抽象便于编写单元测试和 Mock
- **可扩展性**：添加新 DNS 提供商只需实现 trait 并注册

### 开发效率
- **并行开发**：不同模块可以并行开发
- **快速定位**：问题定位更准确
- **代码复用**：通用逻辑可复用

### 用户体验
- **更好的错误信息**：结构化错误类型
- **配置验证**：启动时验证配置
- **文档完善**：自动生成 API 文档

---

## 风险缓解

1. **向后兼容**：保持 CLI 参数和配置文件格式不变
2. **渐进式重构**：分阶段进行，每阶段可独立测试
3. **充分测试**：每阶段完成后运行完整测试
4. **版本控制**：使用 git 分支管理重构过程

---

## 时间估算

| 阶段 | 工作量 | 风险 |
|------|--------|------|
| 阶段一：基础重构 | 1-2 天 | 低 |
| 阶段二：IP 模块 | 2-3 天 | 中 |
| 阶段三：DNS 模块 | 2-3 天 | 中 |
| 阶段四：核心逻辑 | 2-3 天 | 中 |
| 阶段五：测试文档 | 2-3 天 | 低 |
| **总计** | **9-14 天** | - |

---

## 下一步

1. 创建重构分支
2. 执行阶段一（风险最低）
3. 运行测试验证
4. 逐步推进后续阶段
