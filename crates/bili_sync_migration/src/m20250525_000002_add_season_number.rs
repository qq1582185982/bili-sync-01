use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite不支持在一个ALTER TABLE中添加多个列，需要分别执行
        manager
            .alter_table(
                Table::alter()
                    .table(Video::Table)
                    .add_column(ColumnDef::new(Video::SeasonNumber).integer().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Video::Table)
                    .add_column(ColumnDef::new(Video::EpisodeNumber).integer().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Video::Table)
                    .drop_column(Video::SeasonNumber)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Video::Table)
                    .drop_column(Video::EpisodeNumber)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Video {
    Table,
    SeasonNumber,
    EpisodeNumber,
}
