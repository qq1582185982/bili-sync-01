//! 视频源实体定义

use sea_orm::ActiveModelBehavior;
use sea_orm::entity::prelude::*;
use strum::EnumIter;

#[derive(Clone, Debug, PartialEq, Eq, DeriveActiveEnum, EnumIter, Default)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum SourceType {
    #[sea_orm(num_value = 1)]
    #[default]
    Bangumi = 1,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Default)]
#[sea_orm(table_name = "video_source")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub path: String,
    pub r#type: i32,
    pub latest_row_at: DateTime,
    pub season_id: Option<String>,
    pub media_id: Option<String>,
    pub ep_id: Option<String>,
    pub download_all_seasons: Option<bool>,
    pub video_name_template: Option<String>,
    pub page_name_template: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
