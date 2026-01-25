use sea_orm_migration::prelude::*;

/// 旧版迁移占位：仅用于兼容已执行过的历史迁移记录。
/// 新版已有性能索引迁移，不再需要此索引迁移。
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
