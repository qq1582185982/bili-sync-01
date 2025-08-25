use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 创建直播监控配置表
        manager
            .create_table(
                Table::create()
                    .table(LiveMonitor::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(LiveMonitor::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(LiveMonitor::UpperId).big_integer().not_null())
                    .col(ColumnDef::new(LiveMonitor::UpperName).string().not_null())
                    .col(ColumnDef::new(LiveMonitor::RoomId).big_integer().not_null())
                    .col(ColumnDef::new(LiveMonitor::ShortRoomId).big_integer())
                    .col(ColumnDef::new(LiveMonitor::Path).string().not_null())
                    .col(ColumnDef::new(LiveMonitor::Enabled).boolean().default(true).not_null())
                    .col(ColumnDef::new(LiveMonitor::CheckInterval).integer().default(60).not_null())
                    .col(ColumnDef::new(LiveMonitor::Quality).string_len(20).default("high").not_null())
                    .col(ColumnDef::new(LiveMonitor::Format).string_len(10).default("flv").not_null())
                    .col(ColumnDef::new(LiveMonitor::MaxFileSize).big_integer().default(0).not_null())
                    .col(ColumnDef::new(LiveMonitor::LastStatus).integer().default(0).not_null())
                    .col(ColumnDef::new(LiveMonitor::LastCheckAt).string())
                    .col(ColumnDef::new(LiveMonitor::CreatedAt).string().not_null())
                    .to_owned(),
            )
            .await?;

        // 创建直播录制记录表
        manager
            .create_table(
                Table::create()
                    .table(LiveRecord::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(LiveRecord::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(LiveRecord::MonitorId).integer().not_null())
                    .col(ColumnDef::new(LiveRecord::RoomId).big_integer().not_null())
                    .col(ColumnDef::new(LiveRecord::Title).string())
                    .col(ColumnDef::new(LiveRecord::StartTime).string().not_null())
                    .col(ColumnDef::new(LiveRecord::EndTime).string())
                    .col(ColumnDef::new(LiveRecord::FilePath).string())
                    .col(ColumnDef::new(LiveRecord::FileSize).big_integer())
                    .col(ColumnDef::new(LiveRecord::Status).integer().default(0).not_null())
                    .to_owned(),
            )
            .await?;

        // 创建索引以提高查询性能（如果不存在的话）
        // live_monitor表的索引
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_live_monitor_upper_id")
                    .table(LiveMonitor::Table)
                    .col(LiveMonitor::UpperId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_live_monitor_room_id")
                    .table(LiveMonitor::Table)
                    .col(LiveMonitor::RoomId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_live_monitor_enabled_status")
                    .table(LiveMonitor::Table)
                    .col(LiveMonitor::Enabled)
                    .col(LiveMonitor::LastStatus)
                    .to_owned(),
            )
            .await?;

        // live_record表的索引
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_live_record_monitor_id")
                    .table(LiveRecord::Table)
                    .col(LiveRecord::MonitorId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_live_record_room_id")
                    .table(LiveRecord::Table)
                    .col(LiveRecord::RoomId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_live_record_start_time")
                    .table(LiveRecord::Table)
                    .col(LiveRecord::StartTime)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_live_record_status")
                    .table(LiveRecord::Table)
                    .col(LiveRecord::Status)
                    .to_owned(),
            )
            .await?;

        // SQLite不支持在已存在的表上添加外键约束
        // 外键约束的完整性由应用层来保证

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 跳过外键约束删除，因为我们没有创建它们

        // 删除索引
        let indexes_to_drop = [
            "idx_live_monitor_upper_id",
            "idx_live_monitor_room_id", 
            "idx_live_monitor_enabled_status",
            "idx_live_record_monitor_id",
            "idx_live_record_room_id",
            "idx_live_record_start_time",
            "idx_live_record_status",
        ];

        for index_name in indexes_to_drop.iter() {
            manager
                .drop_index(
                    Index::drop()
                        .name(*index_name)
                        .to_owned(),
                )
                .await?;
        }

        // 删除表
        manager
            .drop_table(Table::drop().table(LiveRecord::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(LiveMonitor::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum LiveMonitor {
    Table,
    Id,
    UpperId,
    UpperName,
    RoomId,
    ShortRoomId,
    Path,
    Enabled,
    CheckInterval,
    Quality,
    Format,
    MaxFileSize,
    LastStatus,
    LastCheckAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum LiveRecord {
    Table,
    Id,
    MonitorId,
    RoomId,
    Title,
    StartTime,
    EndTime,
    FilePath,
    FileSize,
    Status,
}