use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use tracing::{debug, error, info, warn};

use crate::config::{Config, ConfigBundle};
use crate::utils::time_format::now_standard_string;
use bili_sync_entity::entities::{config_item, prelude::ConfigItem};

/// 配置管理器，负责配置的数据库存储和热重载
#[derive(Clone)]
pub struct ConfigManager {
    db: DatabaseConnection,
}

impl ConfigManager {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// 确保配置表存在，如果不存在则创建
    pub async fn ensure_tables_exist(&self) -> Result<()> {
        info!("检查配置表是否存在...");

        // 创建config_items表
        let create_config_items = "
            CREATE TABLE IF NOT EXISTS config_items (
                key_name TEXT PRIMARY KEY NOT NULL,
                value_json TEXT NOT NULL,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL
            )";

        // 创建config_changes表
        let create_config_changes = "
            CREATE TABLE IF NOT EXISTS config_changes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key_name TEXT NOT NULL,
                old_value TEXT,
                new_value TEXT NOT NULL,
                changed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL
            )";

        // 执行SQL创建表
        self.db
            .execute_unprepared(create_config_items)
            .await
            .context("创建config_items表失败")?;

        self.db
            .execute_unprepared(create_config_changes)
            .await
            .context("创建config_changes表失败")?;

        info!("配置表检查完成");
        Ok(())
    }

    /// 从数据库加载配置并构建 ConfigBundle
    pub async fn load_config_bundle(&self) -> Result<ConfigBundle> {
        // 尝试从数据库加载配置
        match self.load_from_database().await {
            Ok(config) => {
                info!("从数据库加载配置成功");
                ConfigBundle::from_config(config)
            }
            Err(e) => {
                warn!("从数据库加载配置失败: {}, 尝试从TOML加载", e);
                // 如果数据库加载失败，回退到TOML配置
                let config = self.load_from_toml()?;

                // 将TOML配置迁移到数据库
                if let Err(migrate_err) = self.migrate_to_database(&config).await {
                    error!("迁移配置到数据库失败: {}", migrate_err);
                }

                ConfigBundle::from_config(config)
            }
        }
    }

    /// 从数据库加载配置
    async fn load_from_database(&self) -> Result<Config> {
        let config_items: Vec<config_item::Model> = ConfigItem::find().all(&self.db).await?;

        if config_items.is_empty() {
            return Err(anyhow!("数据库中没有配置项"));
        }

        // 将数据库配置项转换为配置映射
        let mut config_map: HashMap<String, Value> = HashMap::new();
        for item in config_items {
            let value: Value =
                serde_json::from_str(&item.value_json).with_context(|| format!("解析配置项 {} 失败", item.key_name))?;
            config_map.insert(item.key_name, value);
        }

        // 构建完整的配置对象
        self.build_config_from_map(config_map)
    }

    /// 从配置映射构建Config对象
    fn build_config_from_map(&self, config_map: HashMap<String, Value>) -> Result<Config> {
        // 将扁平化的配置映射转换为嵌套结构
        let mut nested_map = serde_json::Map::new();

        for (key, value) in config_map {
            // 处理嵌套键，如 "notification.enable_scan_notifications"
            let parts: Vec<&str> = key.split('.').collect();

            if parts.len() == 1 {
                // 顶级键，直接插入
                nested_map.insert(key, value);
            } else {
                // 嵌套键，需要构建嵌套结构
                Self::insert_nested(&mut nested_map, &parts, value);
            }
        }

        // 将嵌套映射转换为配置对象
        let config_json = Value::Object(nested_map);
        let config: Config = serde_json::from_value(config_json).context("从数据库数据构建配置对象失败")?;

        Ok(config)
    }

    /// 递归插入嵌套值
    fn insert_nested(map: &mut serde_json::Map<String, Value>, parts: &[&str], value: Value) {
        if parts.is_empty() {
            return;
        }

        if parts.len() == 1 {
            map.insert(parts[0].to_string(), value);
            return;
        }

        let key = parts[0];
        let remaining = &parts[1..];

        // 确保当前键存在且是对象
        if !map.contains_key(key) {
            map.insert(key.to_string(), Value::Object(serde_json::Map::new()));
        }

        // 递归处理剩余部分
        if let Some(Value::Object(nested)) = map.get_mut(key) {
            Self::insert_nested(nested, remaining, value);
        }
    }

    /// 移除TOML文件加载 - 配置现在完全基于数据库
    fn load_from_toml(&self) -> Result<Config> {
        // 配置现在完全基于数据库，不再从TOML文件加载
        warn!("TOML配置已弃用，使用默认配置");
        Ok(Config::default())
    }

    /// 将配置保存到数据库
    pub async fn save_config(&self, config: &Config) -> Result<()> {
        // 将配置对象序列化为键值对
        let config_json = serde_json::to_value(config)?;
        let config_map = self.flatten_config_json(config_json)?;

        // 保存到数据库
        for (key, value) in config_map {
            let value_json = serde_json::to_string(&value)?;

            // 查找现有配置项
            let existing = ConfigItem::find()
                .filter(config_item::Column::KeyName.eq(&key))
                .one(&self.db)
                .await?;

            if let Some(existing_model) = existing {
                // 记录变更历史
                if let Err(e) = self
                    .record_config_change(&key, Some(&existing_model.value_json), &value_json)
                    .await
                {
                    warn!("记录配置变更历史失败: {}", e);
                }

                // 更新现有配置项
                let mut active_model: config_item::ActiveModel = existing_model.into();
                active_model.value_json = Set(value_json);
                active_model.updated_at = Set(now_standard_string());
                active_model.update(&self.db).await?;
            } else {
                // 记录变更历史（新增）
                if let Err(e) = self.record_config_change(&key, None, &value_json).await {
                    warn!("记录配置变更历史失败: {}", e);
                }

                // 创建新配置项
                let new_model = config_item::ActiveModel {
                    key_name: Set(key),
                    value_json: Set(value_json),
                    updated_at: Set(now_standard_string()),
                };
                new_model.insert(&self.db).await?;
            }
        }

        info!("配置已保存到数据库");
        Ok(())
    }

    /// 更新单个配置项
    pub async fn update_config_item(&self, key: &str, value: Value) -> Result<()> {
        let value_json = serde_json::to_string(&value)?;

        // 查找现有配置项
        let existing = ConfigItem::find()
            .filter(config_item::Column::KeyName.eq(key))
            .one(&self.db)
            .await?;

        if let Some(existing_model) = existing {
            // 记录变更历史
            if let Err(e) = self
                .record_config_change(key, Some(&existing_model.value_json), &value_json)
                .await
            {
                warn!("记录配置变更历史失败: {}", e);
            }

            // 更新现有配置项
            let mut active_model: config_item::ActiveModel = existing_model.into();
            active_model.value_json = Set(value_json);
            active_model.updated_at = Set(now_standard_string());
            active_model.update(&self.db).await?;
        } else {
            // 记录变更历史
            if let Err(e) = self.record_config_change(key, None, &value_json).await {
                warn!("记录配置变更历史失败: {}", e);
            }

            // 创建新配置项
            let new_model = config_item::ActiveModel {
                key_name: Set(key.to_string()),
                value_json: Set(value_json),
                updated_at: Set(now_standard_string()),
            };
            new_model.insert(&self.db).await?;
        }

        debug!("配置项 {} 已更新", key);
        Ok(())
    }

    /// 将TOML配置迁移到数据库
    async fn migrate_to_database(&self, config: &Config) -> Result<()> {
        info!("开始迁移TOML配置到数据库");
        self.save_config(config).await?;
        info!("TOML配置迁移完成");
        Ok(())
    }

    /// 扁平化配置JSON为键值对
    fn flatten_config_json(&self, config_json: Value) -> Result<HashMap<String, Value>> {
        let mut result = HashMap::new();

        if let Value::Object(map) = config_json {
            for (key, value) in map {
                // 对于复杂对象，直接存储整个JSON值
                result.insert(key, value);
            }
        } else {
            return Err(anyhow!("配置必须是JSON对象"));
        }

        Ok(result)
    }

    /// 记录配置变更历史 (使用原生SQL)
    async fn record_config_change(&self, key: &str, old_value: Option<&str>, new_value: &str) -> Result<()> {
        let sql = "INSERT INTO config_changes (key_name, old_value, new_value, changed_at) VALUES (?, ?, ?, ?)";

        let stmt = sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            sql,
            vec![
                key.into(),
                old_value.into(),
                new_value.into(),
                now_standard_string().into(),
            ],
        );
        self.db.execute(stmt).await?;

        Ok(())
    }

    /// 获取配置变更历史 (使用原生SQL)
    pub async fn get_config_history(
        &self,
        key: Option<&str>,
        limit: Option<u64>,
    ) -> Result<Vec<config_item::ConfigChangeModel>> {
        let mut sql = "SELECT id, key_name, old_value, new_value, changed_at FROM config_changes".to_string();
        let mut values = Vec::new();

        if let Some(key) = key {
            sql.push_str(" WHERE key_name = ?");
            values.push(key.into());
        }

        sql.push_str(" ORDER BY changed_at DESC");

        if let Some(limit) = limit {
            sql.push_str(" LIMIT ?");
            values.push(limit.into());
        }

        let stmt = sea_orm::Statement::from_sql_and_values(sea_orm::DatabaseBackend::Sqlite, &sql, values);

        let query_result = self.db.query_all(stmt).await?;

        let mut changes = Vec::new();
        for row in query_result {
            let change = config_item::ConfigChangeModel {
                id: row.try_get::<i32>("", "id")?,
                key_name: row.try_get::<String>("", "key_name")?,
                old_value: row.try_get::<Option<String>>("", "old_value")?,
                new_value: row.try_get::<String>("", "new_value")?,
                changed_at: row.try_get::<String>("", "changed_at")?,
            };
            changes.push(change);
        }

        Ok(changes)
    }
}
