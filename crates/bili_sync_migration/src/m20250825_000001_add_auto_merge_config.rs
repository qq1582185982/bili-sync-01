use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 插入直播录制自动合并配置的默认值
        let insert_stmt = Query::insert()
            .into_table(ConfigItem::Table)
            .columns([
                ConfigItem::KeyName,
                ConfigItem::ValueJson,
                ConfigItem::UpdatedAt,
            ])
            .values_panic([
                "live_recording_config".into(),
                r#"{
                    "auto_merge": {
                        "enabled": false,
                        "duration_threshold": 600,
                        "keep_segments_after_merge": false,
                        "output_format": "mp4",
                        "output_quality": "Auto"
                    },
                    "quality": {
                        "preferred_format": "flv",
                        "resolution": "1080p",
                        "frame_rate": 30
                    },
                    "file_management": {
                        "max_segments_to_keep": 50,
                        "filename_template": "{upper_name}_{room_id}_{date}_{time}_{title}.{ext}",
                        "auto_cleanup_days": 7
                    }
                }"#.into(),
"2025-08-25 14:30:00 UTC".into(),
            ])
            .to_owned();

        manager.exec_stmt(insert_stmt).await?;

        // 插入简化的自动合并配置项（向后兼容）
        let insert_auto_merge_enabled = Query::insert()
            .into_table(ConfigItem::Table)
            .columns([
                ConfigItem::KeyName,
                ConfigItem::ValueJson,
                ConfigItem::UpdatedAt,
            ])
            .values_panic([
                "auto_merge_enabled".into(),
                "false".into(),
"2025-08-25 14:30:00 UTC".into(),
            ])
            .to_owned();

        manager.exec_stmt(insert_auto_merge_enabled).await?;

        let insert_auto_merge_duration = Query::insert()
            .into_table(ConfigItem::Table)
            .columns([
                ConfigItem::KeyName,
                ConfigItem::ValueJson,
                ConfigItem::UpdatedAt,
            ])
            .values_panic([
                "auto_merge_duration_seconds".into(),
                "600".into(),
"2025-08-25 14:30:00 UTC".into(),
            ])
            .to_owned();

        manager.exec_stmt(insert_auto_merge_duration).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 删除添加的配置项
        let delete_stmt = Query::delete()
            .from_table(ConfigItem::Table)
            .and_where(
                Expr::col(ConfigItem::KeyName).is_in([
                    "live_recording_config",
                    "auto_merge_enabled", 
                    "auto_merge_duration_seconds"
                ])
            )
            .to_owned();

        manager.exec_stmt(delete_stmt).await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum ConfigItem {
    #[sea_orm(iden = "config_items")]
    Table,
    KeyName,
    ValueJson,
    UpdatedAt,
}