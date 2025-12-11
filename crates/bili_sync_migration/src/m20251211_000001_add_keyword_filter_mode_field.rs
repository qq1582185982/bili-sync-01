use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 为collection表添加keyword_filter_mode字段
        // 默认值为 "blacklist"，可选值：blacklist（排除匹配）、whitelist（只下载匹配）
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .add_column(
                        ColumnDef::new(Collection::KeywordFilterMode)
                            .text()
                            .null()
                            .default("blacklist"),
                    )
                    .to_owned(),
            )
            .await?;

        // 为favorite表添加keyword_filter_mode字段
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .add_column(
                        ColumnDef::new(Favorite::KeywordFilterMode)
                            .text()
                            .null()
                            .default("blacklist"),
                    )
                    .to_owned(),
            )
            .await?;

        // 为submission表添加keyword_filter_mode字段
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .add_column(
                        ColumnDef::new(Submission::KeywordFilterMode)
                            .text()
                            .null()
                            .default("blacklist"),
                    )
                    .to_owned(),
            )
            .await?;

        // 为watch_later表添加keyword_filter_mode字段
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .add_column(
                        ColumnDef::new(WatchLater::KeywordFilterMode)
                            .text()
                            .null()
                            .default("blacklist"),
                    )
                    .to_owned(),
            )
            .await?;

        // 为video_source表添加keyword_filter_mode字段
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .add_column(
                        ColumnDef::new(VideoSource::KeywordFilterMode)
                            .text()
                            .null()
                            .default("blacklist"),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 回滚：删除keyword_filter_mode字段
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .drop_column(Collection::KeywordFilterMode)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .drop_column(Favorite::KeywordFilterMode)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .drop_column(Submission::KeywordFilterMode)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .drop_column(WatchLater::KeywordFilterMode)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .drop_column(VideoSource::KeywordFilterMode)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Collection {
    Table,
    KeywordFilterMode,
}

#[derive(DeriveIden)]
enum Favorite {
    Table,
    KeywordFilterMode,
}

#[derive(DeriveIden)]
enum Submission {
    Table,
    KeywordFilterMode,
}

#[derive(DeriveIden)]
enum WatchLater {
    Table,
    KeywordFilterMode,
}

#[derive(DeriveIden)]
enum VideoSource {
    Table,
    KeywordFilterMode,
}
