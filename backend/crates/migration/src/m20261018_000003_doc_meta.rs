//! P1 阶段 3：front matter 元数据（tags / description）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum DocMeta {
    Table,
    VersionId,
    DocumentId,
    Description,
    CreatedAt,
}

#[derive(DeriveIden)]
enum DocTags {
    Table,
    VersionId,
    Tag,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DocMeta::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DocMeta::VersionId)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DocMeta::DocumentId).uuid().not_null())
                    .col(ColumnDef::new(DocMeta::Description).text())
                    .col(
                        ColumnDef::new(DocMeta::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(DocTags::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(DocTags::VersionId).uuid().not_null())
                    .col(ColumnDef::new(DocTags::Tag).string_len(64).not_null())
                    .primary_key(
                        Index::create()
                            .col(DocTags::VersionId)
                            .col(DocTags::Tag),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-doc-tags-tag")
                    .table(DocTags::Table)
                    .col(DocTags::Tag)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DocTags::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DocMeta::Table).to_owned())
            .await?;
        Ok(())
    }
}
