//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.15

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Default)]
#[sea_orm(table_name = "video")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub collection_id: Option<i32>,
    pub favorite_id: Option<i32>,
    pub watch_later_id: Option<i32>,
    pub submission_id: Option<i32>,
    pub source_id: Option<i32>,
    pub source_type: Option<i32>,
    pub upper_id: i64,
    pub upper_name: String,
    pub upper_face: String,
    pub staff_info: Option<serde_json::Value>,
    pub source_submission_id: Option<i32>,
    pub name: String,
    pub path: String,
    pub category: i32,
    pub bvid: String,
    pub intro: String,
    pub cover: String,
    pub ctime: DateTime,
    pub pubtime: DateTime,
    pub favtime: DateTime,
    pub download_status: u32,
    pub valid: bool,
    pub tags: Option<serde_json::Value>,
    pub single_page: Option<bool>,
    pub created_at: String,
    pub season_id: Option<String>,
    pub ep_id: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub deleted: i32,
    pub share_copy: Option<String>,
    pub show_season_type: Option<i32>,
    pub actors: Option<String>,
    pub auto_download: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::page::Entity")]
    Page,
}

impl Related<super::page::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Page.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
