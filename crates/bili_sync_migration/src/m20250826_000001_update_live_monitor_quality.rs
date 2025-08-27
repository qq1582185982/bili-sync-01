use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 添加新的质量等级字段
        manager
            .alter_table(
                Table::alter()
                    .table(LiveMonitor::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(LiveMonitor::QualityLevel)
                            .unsigned()
                            .default(10000u32) // 默认为原画
                            .comment("B站质量等级 (qn)")
                    )
                    .to_owned(),
            )
            .await?;

        // 将现有的质量字符串转换为质量等级数字
        // 这里需要执行SQL来更新现有数据
        let update_sql = r#"
            UPDATE live_monitor 
            SET quality_level = CASE 
                WHEN quality LIKE '%原画%' OR quality = 'original' THEN 10000
                WHEN quality LIKE '%4K%' OR quality = '4k' THEN 800
                WHEN quality LIKE '%蓝光杜比%' OR quality = 'bluray_dolby' THEN 401
                WHEN quality LIKE '%蓝光%' OR quality = 'bluray' THEN 400
                WHEN quality LIKE '%超清%' OR quality = 'super_high' THEN 250
                WHEN quality LIKE '%高清%' OR quality = 'high' THEN 150
                WHEN quality LIKE '%流畅%' OR quality = 'smooth' THEN 80
                ELSE 10000 -- 默认原画
            END
        "#;
        
        manager.get_connection().execute_unprepared(update_sql).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 删除质量等级字段
        manager
            .alter_table(
                Table::alter()
                    .table(LiveMonitor::Table)
                    .drop_column(LiveMonitor::QualityLevel)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum LiveMonitor {
    Table,
    QualityLevel,
}