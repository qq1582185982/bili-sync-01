use sea_orm_migration::prelude::*;

/// 旧版迁移占位：仅用于兼容已执行过的历史迁移记录。
/// 新版已通过其它迁移覆盖同等字段，不再需要重复执行。
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
