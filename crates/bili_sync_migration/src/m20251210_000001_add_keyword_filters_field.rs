use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 为collection表添加keyword_filters字段
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .add_column(ColumnDef::new(Collection::KeywordFilters).text().null())
                    .to_owned(),
            )
            .await?;

        // 为favorite表添加keyword_filters字段
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .add_column(ColumnDef::new(Favorite::KeywordFilters).text().null())
                    .to_owned(),
            )
            .await?;

        // 为submission表添加keyword_filters字段
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .add_column(ColumnDef::new(Submission::KeywordFilters).text().null())
                    .to_owned(),
            )
            .await?;

        // 为watch_later表添加keyword_filters字段
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .add_column(ColumnDef::new(WatchLater::KeywordFilters).text().null())
                    .to_owned(),
            )
            .await?;

        // 为video_source表添加keyword_filters字段
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .add_column(ColumnDef::new(VideoSource::KeywordFilters).text().null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 回滚：删除keyword_filters字段
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .drop_column(Collection::KeywordFilters)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .drop_column(Favorite::KeywordFilters)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .drop_column(Submission::KeywordFilters)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .drop_column(WatchLater::KeywordFilters)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .drop_column(VideoSource::KeywordFilters)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Collection {
    Table,
    KeywordFilters,
}

#[derive(DeriveIden)]
enum Favorite {
    Table,
    KeywordFilters,
}

#[derive(DeriveIden)]
enum Submission {
    Table,
    KeywordFilters,
}

#[derive(DeriveIden)]
enum WatchLater {
    Table,
    KeywordFilters,
}

#[derive(DeriveIden)]
enum VideoSource {
    Table,
    KeywordFilters,
}
