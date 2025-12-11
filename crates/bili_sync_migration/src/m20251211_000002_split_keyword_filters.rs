use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 为 collection 表添加 blacklist_keywords 和 whitelist_keywords 字段
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .add_column(ColumnDef::new(Collection::BlacklistKeywords).text().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .add_column(ColumnDef::new(Collection::WhitelistKeywords).text().null())
                    .to_owned(),
            )
            .await?;

        // 为 favorite 表添加字段
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .add_column(ColumnDef::new(Favorite::BlacklistKeywords).text().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .add_column(ColumnDef::new(Favorite::WhitelistKeywords).text().null())
                    .to_owned(),
            )
            .await?;

        // 为 submission 表添加字段
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .add_column(ColumnDef::new(Submission::BlacklistKeywords).text().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .add_column(ColumnDef::new(Submission::WhitelistKeywords).text().null())
                    .to_owned(),
            )
            .await?;

        // 为 watch_later 表添加字段
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .add_column(ColumnDef::new(WatchLater::BlacklistKeywords).text().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .add_column(ColumnDef::new(WatchLater::WhitelistKeywords).text().null())
                    .to_owned(),
            )
            .await?;

        // 为 video_source 表添加字段
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .add_column(ColumnDef::new(VideoSource::BlacklistKeywords).text().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .add_column(ColumnDef::new(VideoSource::WhitelistKeywords).text().null())
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
                    .drop_column(Collection::BlacklistKeywords)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .drop_column(Collection::WhitelistKeywords)
                    .to_owned(),
            )
            .await?;

        // 删除 favorite 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .drop_column(Favorite::BlacklistKeywords)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .drop_column(Favorite::WhitelistKeywords)
                    .to_owned(),
            )
            .await?;

        // 删除 submission 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .drop_column(Submission::BlacklistKeywords)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .drop_column(Submission::WhitelistKeywords)
                    .to_owned(),
            )
            .await?;

        // 删除 watch_later 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .drop_column(WatchLater::BlacklistKeywords)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .drop_column(WatchLater::WhitelistKeywords)
                    .to_owned(),
            )
            .await?;

        // 删除 video_source 表的字段
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .drop_column(VideoSource::BlacklistKeywords)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .drop_column(VideoSource::WhitelistKeywords)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Collection {
    Table,
    BlacklistKeywords,
    WhitelistKeywords,
}

#[derive(Iden)]
enum Favorite {
    Table,
    BlacklistKeywords,
    WhitelistKeywords,
}

#[derive(Iden)]
enum Submission {
    Table,
    BlacklistKeywords,
    WhitelistKeywords,
}

#[derive(Iden)]
enum WatchLater {
    Table,
    BlacklistKeywords,
    WhitelistKeywords,
}

#[derive(Iden)]
enum VideoSource {
    Table,
    BlacklistKeywords,
    WhitelistKeywords,
}
