use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("video_source"))
                    .add_column(ColumnDef::new(Alias::new("video_name_template")).string().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("video_source"))
                    .add_column(ColumnDef::new(Alias::new("page_name_template")).string().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("video_source"))
                    .drop_column(Alias::new("video_name_template"))
                    .drop_column(Alias::new("page_name_template"))
                    .to_owned(),
            )
            .await
    }
}
