//! 配置加载器

use crate::config::model::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 配置加载错误
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Failed to parse config JSON: {0}")]
    JsonError(#[from] serde_json::Error),
    
    #[error("Environment variable not found: {0}")]
    EnvVarNotFound(String),
}

/// 配置加载器
pub struct ConfigLoader {
    _base_dir: Option<PathBuf>,
}

impl ConfigLoader {
    pub fn new() -> Self {
        Self { _base_dir: None }
    }

    /// 从文件加载配置
    pub fn load<P: AsRef<Path>>(&self, path: P) -> Result<Config, ConfigError> {
        let content = fs::read_to_string(path.as_ref())?;
        let mut config: Config = serde_json::from_str(&content)?;
        
        // 展开环境变量引用
        Self::expand_env_vars(&mut config)?;
        
        Ok(config)
    }

    /// 展开配置中的环境变量引用
    fn expand_env_vars(config: &mut Config) -> Result<(), ConfigError> {
        for record in &mut config.records {
            if let Some(ref mut cf) = record.cloudflare {
                if cf.api_token.starts_with('$') {
                    cf.api_token = Self::expand_single_var(&cf.api_token, &config.environment)?;
                }
                if cf.zone_id.starts_with('$') {
                    cf.zone_id = Self::expand_single_var(&cf.zone_id, &config.environment)?;
                }
            }
            if let Some(ref mut aliyun) = record.aliyun {
                if aliyun.access_key_id.starts_with('$') {
                    aliyun.access_key_id = Self::expand_single_var(&aliyun.access_key_id, &config.environment)?;
                }
                if aliyun.access_key_secret.starts_with('$') {
                    aliyun.access_key_secret = Self::expand_single_var(&aliyun.access_key_secret, &config.environment)?;
                }
            }
        }
        Ok(())
    }

    /// 展开单个变量引用
    fn expand_single_var(value: &str, env: &HashMap<String, String>) -> Result<String, ConfigError> {
        if !value.starts_with('$') {
            return Ok(value.to_string());
        }

        let var_name = &value[1..];
        env.get(var_name)
            .cloned()
            .ok_or_else(|| ConfigError::EnvVarNotFound(var_name.to_string()))
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// 便捷函数：从文件加载配置
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config, ConfigError> {
    ConfigLoader::new().load(path)
}
