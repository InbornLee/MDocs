//! P1 阶段 2：chunk 向量表（混合检索）。
//!
//! 向量以 f32 小端字节存 BLOB/bytea，避免依赖 sqlite-vec / pgvector；
//! 检索在应用层做余弦相似度，规模增大后再换 ANN 索引。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum ChunkVectors {
    Table,
    ChunkId,
    Model,
    Dim,
    Vector,
    CreatedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ChunkVectors::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ChunkVectors::ChunkId)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ChunkVectors::Model)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(ChunkVectors::Dim).integer().not_null())
                    .col(ColumnDef::new(ChunkVectors::Vector).binary().not_null())
                    .col(
                        ColumnDef::new(ChunkVectors::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-chunk-vectors-model")
                    .table(ChunkVectors::Table)
                    .col(ChunkVectors::Model)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ChunkVectors::Table).to_owned())
            .await?;
        Ok(())
    }
}
