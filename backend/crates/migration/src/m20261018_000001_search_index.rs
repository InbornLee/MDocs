//! P1 阶段 1：chunk 级倒排索引。
//!
//! 索引按 `doc_versions` 建（版本不可变），搜索时用当前分支 head 的版本集合过滤；
//! `chunk_terms` 存 term -> chunk 的 tf，配合应用层 BM25 打分，SQLite / PostgreSQL 通用。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum DocChunks {
    Table,
    Id,
    VersionId,
    DocumentId,
    Ord,
    Heading,
    Anchor,
    Level,
    StartLine,
    EndLine,
    Text,
    TokenCount,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ChunkTerms {
    Table,
    Id,
    ChunkId,
    Term,
    Tf,
}

#[derive(DeriveIden)]
enum SearchIndexState {
    Table,
    VersionId,
    ChunkCount,
    IndexedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DocChunks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(DocChunks::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(DocChunks::VersionId).uuid().not_null())
                    .col(ColumnDef::new(DocChunks::DocumentId).uuid().not_null())
                    .col(ColumnDef::new(DocChunks::Ord).integer().not_null().default(0))
                    .col(ColumnDef::new(DocChunks::Heading).string_len(512))
                    .col(ColumnDef::new(DocChunks::Anchor).string_len(512))
                    .col(ColumnDef::new(DocChunks::Level).integer())
                    .col(ColumnDef::new(DocChunks::StartLine).integer().not_null().default(1))
                    .col(ColumnDef::new(DocChunks::EndLine).integer().not_null().default(1))
                    .col(ColumnDef::new(DocChunks::Text).text().not_null())
                    .col(ColumnDef::new(DocChunks::TokenCount).integer().not_null().default(0))
                    .col(
                        ColumnDef::new(DocChunks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-doc-chunks-version")
                    .table(DocChunks::Table)
                    .col(DocChunks::VersionId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-doc-chunks-document")
                    .table(DocChunks::Table)
                    .col(DocChunks::DocumentId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ChunkTerms::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ChunkTerms::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(ChunkTerms::ChunkId).uuid().not_null())
                    .col(ColumnDef::new(ChunkTerms::Term).string_len(64).not_null())
                    .col(ColumnDef::new(ChunkTerms::Tf).integer().not_null().default(1))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-chunk-terms-term")
                    .table(ChunkTerms::Table)
                    .col(ChunkTerms::Term)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-chunk-terms-chunk")
                    .table(ChunkTerms::Table)
                    .col(ChunkTerms::ChunkId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-chunk-terms-unique")
                    .table(ChunkTerms::Table)
                    .col(ChunkTerms::ChunkId)
                    .col(ChunkTerms::Term)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SearchIndexState::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SearchIndexState::VersionId)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SearchIndexState::ChunkCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SearchIndexState::IndexedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SearchIndexState::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ChunkTerms::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DocChunks::Table).to_owned())
            .await?;
        Ok(())
    }
}
