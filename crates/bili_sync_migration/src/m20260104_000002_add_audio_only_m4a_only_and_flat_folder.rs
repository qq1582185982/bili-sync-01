use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Collection: audio_only_m4a_only
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .add_column(ColumnDef::new(Collection::AudioOnlyM4aOnly).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // Collection: flat_folder
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .add_column(ColumnDef::new(Collection::FlatFolder).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // Favorite: audio_only_m4a_only
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .add_column(ColumnDef::new(Favorite::AudioOnlyM4aOnly).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // Favorite: flat_folder
        manager
            .alter_table(
                Table::alter()
                    .table(Favorite::Table)
                    .add_column(ColumnDef::new(Favorite::FlatFolder).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // Submission: audio_only_m4a_only
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .add_column(ColumnDef::new(Submission::AudioOnlyM4aOnly).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // Submission: flat_folder
        manager
            .alter_table(
                Table::alter()
                    .table(Submission::Table)
                    .add_column(ColumnDef::new(Submission::FlatFolder).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // WatchLater: audio_only_m4a_only
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .add_column(ColumnDef::new(WatchLater::AudioOnlyM4aOnly).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // WatchLater: flat_folder
        manager
            .alter_table(
                Table::alter()
                    .table(WatchLater::Table)
                    .add_column(ColumnDef::new(WatchLater::FlatFolder).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // VideoSource: audio_only_m4a_only
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .add_column(ColumnDef::new(VideoSource::AudioOnlyM4aOnly).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // VideoSource: flat_folder
        manager
            .alter_table(
                Table::alter()
                    .table(VideoSource::Table)
                    .add_column(ColumnDef::new(VideoSource::FlatFolder).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Collection
        manager
            .alter_table(Table::alter().table(Collection::Table).drop_column(Collection::AudioOnlyM4aOnly).to_owned())
            .await?;
        manager
            .alter_table(Table::alter().table(Collection::Table).drop_column(Collection::FlatFolder).to_owned())
            .await?;

        // Favorite
        manager
            .alter_table(Table::alter().table(Favorite::Table).drop_column(Favorite::AudioOnlyM4aOnly).to_owned())
            .await?;
        manager
            .alter_table(Table::alter().table(Favorite::Table).drop_column(Favorite::FlatFolder).to_owned())
            .await?;

        // Submission
        manager
            .alter_table(Table::alter().table(Submission::Table).drop_column(Submission::AudioOnlyM4aOnly).to_owned())
            .await?;
        manager
            .alter_table(Table::alter().table(Submission::Table).drop_column(Submission::FlatFolder).to_owned())
            .await?;

        // WatchLater
        manager
            .alter_table(Table::alter().table(WatchLater::Table).drop_column(WatchLater::AudioOnlyM4aOnly).to_owned())
            .await?;
        manager
            .alter_table(Table::alter().table(WatchLater::Table).drop_column(WatchLater::FlatFolder).to_owned())
            .await?;

        // VideoSource
        manager
            .alter_table(Table::alter().table(VideoSource::Table).drop_column(VideoSource::AudioOnlyM4aOnly).to_owned())
            .await?;
        manager
            .alter_table(Table::alter().table(VideoSource::Table).drop_column(VideoSource::FlatFolder).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Collection {
    Table,
    AudioOnlyM4aOnly,
    FlatFolder,
}

#[derive(DeriveIden)]
enum Favorite {
    Table,
    AudioOnlyM4aOnly,
    FlatFolder,
}

#[derive(DeriveIden)]
enum Submission {
    Table,
    AudioOnlyM4aOnly,
    FlatFolder,
}

#[derive(DeriveIden)]
enum WatchLater {
    Table,
    AudioOnlyM4aOnly,
    FlatFolder,
}

#[derive(DeriveIden)]
enum VideoSource {
    Table,
    AudioOnlyM4aOnly,
    FlatFolder,
}
