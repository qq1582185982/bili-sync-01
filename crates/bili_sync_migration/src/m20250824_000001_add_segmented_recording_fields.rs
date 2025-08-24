use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 添加分段录制相关字段到 live_record 表
        manager
            .alter_table(
                Table::alter()
                    .table(LiveRecord::Table)
                    .add_column(
                        ColumnDef::new(LiveRecord::SegmentMode)
                            .boolean()
                            .default(true)
                            .not_null()
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(LiveRecord::Table)
                    .add_column(
                        ColumnDef::new(LiveRecord::SegmentCount)
                            .integer()
                            .default(0)
                            .not_null()
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(LiveRecord::Table)
                    .add_column(
                        ColumnDef::new(LiveRecord::UrlSwitchCount)
                            .integer()
                            .default(0)
                            .not_null()
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(LiveRecord::Table)
                    .add_column(
                        ColumnDef::new(LiveRecord::CdnNodes)
                            .text()
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 删除添加的字段
        manager
            .alter_table(
                Table::alter()
                    .table(LiveRecord::Table)
                    .drop_column(LiveRecord::CdnNodes)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(LiveRecord::Table)
                    .drop_column(LiveRecord::UrlSwitchCount)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(LiveRecord::Table)
                    .drop_column(LiveRecord::SegmentCount)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(LiveRecord::Table)
                    .drop_column(LiveRecord::SegmentMode)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum LiveRecord {
    Table,
    SegmentMode,
    SegmentCount,
    UrlSwitchCount,
    CdnNodes,
}