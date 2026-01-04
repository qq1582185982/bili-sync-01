use sea_orm::entity::prelude::*;

/// AI对话历史实体
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "ai_conversation_history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub source_key: String,
    pub role: String,
    #[sea_orm(column_type = "Text")]
    pub content: String,
    pub order_index: i32,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
