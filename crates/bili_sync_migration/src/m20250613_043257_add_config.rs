use sea_orm_migration::prelude::*;

/// 旧版迁移占位：仅用于兼容已执行过的历史迁移记录。
/// 新版已使用 config_items/config_changes 结构，不再依赖旧 config 表。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
