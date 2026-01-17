use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 创建AI对话历史表
        manager
            .create_table(
                Table::create()
                    .table(AiConversationHistory::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AiConversationHistory::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AiConversationHistory::SourceKey).string().not_null())
                    .col(ColumnDef::new(AiConversationHistory::Role).string().not_null())
                    .col(ColumnDef::new(AiConversationHistory::Content).text().not_null())
                    .col(ColumnDef::new(AiConversationHistory::OrderIndex).integer().not_null())
                    .col(
                        ColumnDef::new(AiConversationHistory::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 创建索引以加速按source_key查询
        manager
            .create_index(
                Index::create()
                    .name("idx_ai_conversation_source_key")
                    .table(AiConversationHistory::Table)
                    .col(AiConversationHistory::SourceKey)
                    .to_owned(),
            )
            .await?;

        // 创建复合索引以加速排序查询
        manager
            .create_index(
                Index::create()
                    .name("idx_ai_conversation_source_order")
                    .table(AiConversationHistory::Table)
                    .col(AiConversationHistory::SourceKey)
                    .col(AiConversationHistory::OrderIndex)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AiConversationHistory::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum AiConversationHistory {
    Table,
    Id,
    SourceKey,
    Role,
    Content,
    OrderIndex,
    CreatedAt,
}
