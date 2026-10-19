//! 放宽链接/标题字段，避免长锚点或长标题在 PostgreSQL 上触发
//! `value too long for type character varying(255)`。
//!
//! SQLite 不限制 varchar 长度，跳过 ALTER；写入侧仍会做字符截断。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() == DbBackend::Postgres {
            let db = manager.get_connection();
            for sql in [
                "ALTER TABLE link_edges ALTER COLUMN anchor TYPE varchar(1024)",
                "ALTER TABLE link_edges ALTER COLUMN display_text TYPE varchar(1024)",
                "ALTER TABLE link_edges ALTER COLUMN raw_target TYPE varchar(2048)",
                "ALTER TABLE documents ALTER COLUMN title TYPE varchar(512)",
                "ALTER TABLE doc_chunks ALTER COLUMN heading TYPE varchar(1024)",
                "ALTER TABLE doc_chunks ALTER COLUMN anchor TYPE varchar(1024)",
            ] {
                db.execute_unprepared(sql).await?;
            }
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // 收窄字段可能因现有数据超长而失败，保留为 no-op。
        Ok(())
    }
}
