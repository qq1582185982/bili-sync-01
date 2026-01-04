use sea_orm_migration::prelude::*;
use sea_orm::ConnectionTrait;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite 不支持一次添加多列，需要分开添加
        // 使用原生SQL检查列是否存在，避免重复添加

        let db = manager.get_connection();

        // 为收藏夹表添加AI重命名提示词字段
        if !column_exists(db, "favorite", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Favorite::Table)
                        .add_column(
                            ColumnDef::new(Favorite::AiRenameVideoPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        if !column_exists(db, "favorite", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Favorite::Table)
                        .add_column(
                            ColumnDef::new(Favorite::AiRenameAudioPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        // 为合集表添加AI重命名提示词字段
        if !column_exists(db, "collection", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Collection::Table)
                        .add_column(
                            ColumnDef::new(Collection::AiRenameVideoPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        if !column_exists(db, "collection", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Collection::Table)
                        .add_column(
                            ColumnDef::new(Collection::AiRenameAudioPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        // 为UP主投稿表添加AI重命名提示词字段
        if !column_exists(db, "submission", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Submission::Table)
                        .add_column(
                            ColumnDef::new(Submission::AiRenameVideoPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        if !column_exists(db, "submission", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Submission::Table)
                        .add_column(
                            ColumnDef::new(Submission::AiRenameAudioPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        // 为稍后再看表添加AI重命名提示词字段
        if !column_exists(db, "watch_later", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(WatchLater::Table)
                        .add_column(
                            ColumnDef::new(WatchLater::AiRenameVideoPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        if !column_exists(db, "watch_later", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(WatchLater::Table)
                        .add_column(
                            ColumnDef::new(WatchLater::AiRenameAudioPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        // 为番剧表添加AI重命名提示词字段
        if !column_exists(db, "video_source", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(VideoSource::Table)
                        .add_column(
                            ColumnDef::new(VideoSource::AiRenameVideoPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        if !column_exists(db, "video_source", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(VideoSource::Table)
                        .add_column(
                            ColumnDef::new(VideoSource::AiRenameAudioPrompt)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // 删除收藏夹表的AI重命名提示词字段
        if column_exists(db, "favorite", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Favorite::Table)
                        .drop_column(Favorite::AiRenameVideoPrompt)
                        .to_owned(),
                )
                .await?;
        }

        if column_exists(db, "favorite", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Favorite::Table)
                        .drop_column(Favorite::AiRenameAudioPrompt)
                        .to_owned(),
                )
                .await?;
        }

        // 删除合集表的AI重命名提示词字段
        if column_exists(db, "collection", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Collection::Table)
                        .drop_column(Collection::AiRenameVideoPrompt)
                        .to_owned(),
                )
                .await?;
        }

        if column_exists(db, "collection", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Collection::Table)
                        .drop_column(Collection::AiRenameAudioPrompt)
                        .to_owned(),
                )
                .await?;
        }

        // 删除UP主投稿表的AI重命名提示词字段
        if column_exists(db, "submission", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Submission::Table)
                        .drop_column(Submission::AiRenameVideoPrompt)
                        .to_owned(),
                )
                .await?;
        }

        if column_exists(db, "submission", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Submission::Table)
                        .drop_column(Submission::AiRenameAudioPrompt)
                        .to_owned(),
                )
                .await?;
        }

        // 删除稍后再看表的AI重命名提示词字段
        if column_exists(db, "watch_later", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(WatchLater::Table)
                        .drop_column(WatchLater::AiRenameVideoPrompt)
                        .to_owned(),
                )
                .await?;
        }

        if column_exists(db, "watch_later", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(WatchLater::Table)
                        .drop_column(WatchLater::AiRenameAudioPrompt)
                        .to_owned(),
                )
                .await?;
        }

        // 删除番剧表的AI重命名提示词字段
        if column_exists(db, "video_source", "ai_rename_video_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(VideoSource::Table)
                        .drop_column(VideoSource::AiRenameVideoPrompt)
                        .to_owned(),
                )
                .await?;
        }

        if column_exists(db, "video_source", "ai_rename_audio_prompt").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(VideoSource::Table)
                        .drop_column(VideoSource::AiRenameAudioPrompt)
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }
}

/// 检查SQLite表中是否存在指定列
async fn column_exists<C: ConnectionTrait>(
    db: &C,
    table_name: &str,
    column_name: &str,
) -> Result<bool, DbErr> {
    use sea_orm::Statement;

    let sql = format!("PRAGMA table_info({})", table_name);
    let result = db
        .query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            sql,
        ))
        .await?;

    for row in result {
        let name: String = row.try_get("", "name")?;
        if name == column_name {
            return Ok(true);
        }
    }

    Ok(false)
}

#[derive(Iden)]
enum Favorite {
    Table,
    AiRenameVideoPrompt,
    AiRenameAudioPrompt,
}

#[derive(Iden)]
enum Collection {
    Table,
    AiRenameVideoPrompt,
    AiRenameAudioPrompt,
}

#[derive(Iden)]
enum Submission {
    Table,
    AiRenameVideoPrompt,
    AiRenameAudioPrompt,
}

#[derive(Iden)]
enum WatchLater {
    Table,
    AiRenameVideoPrompt,
    AiRenameAudioPrompt,
}

#[derive(Iden)]
enum VideoSource {
    Table,
    AiRenameVideoPrompt,
    AiRenameAudioPrompt,
}
