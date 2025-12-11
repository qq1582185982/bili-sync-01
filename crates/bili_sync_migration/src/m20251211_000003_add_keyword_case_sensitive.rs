use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 为 collection 表添加 keyword_case_sensitive 字段（默认为 true，区分大小写）
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .add_column(
                        ColumnDef::new(Collection::KeywordCaseSensitive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        // 为 favorite 表添加字段
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .add_column(
                        ColumnDef::new(Favorite::KeywordCaseSensitive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        // 为 submission 表添加字段
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .add_column(
                        ColumnDef::new(Submission::KeywordCaseSensitive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        // 为 watch_later 表添加字段
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .add_column(
                        ColumnDef::new(WatchLater::KeywordCaseSensitive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        // 为 video_source 表添加字段
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .add_column(
                        ColumnDef::new(VideoSource::KeywordCaseSensitive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 删除 collection 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .drop_column(Collection::KeywordCaseSensitive)
                    .to_owned(),
            )
            .await?;

        // 删除 favorite 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .drop_column(Favorite::KeywordCaseSensitive)
                    .to_owned(),
            )
            .await?;

        // 删除 submission 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .drop_column(Submission::KeywordCaseSensitive)
                    .to_owned(),
            )
            .await?;

        // 删除 watch_later 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .drop_column(WatchLater::KeywordCaseSensitive)
                    .to_owned(),
            )
            .await?;

        // 删除 video_source 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .drop_column(VideoSource::KeywordCaseSensitive)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Collection {
    Table,
    KeywordCaseSensitive,
}

#[derive(Iden)]
enum Favorite {
    Table,
    KeywordCaseSensitive,
}

#[derive(Iden)]
enum Submission {
    Table,
    KeywordCaseSensitive,
}

#[derive(Iden)]
enum WatchLater {
    Table,
    KeywordCaseSensitive,
}

#[derive(Iden)]
enum VideoSource {
    Table,
    KeywordCaseSensitive,
}
