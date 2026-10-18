//! P0 AI 知识库改进：token 过期时间 + MCP 调用审计表。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum ApiTokens {
    Table,
    ExpiresAt,
}

#[derive(DeriveIden)]
enum McpAuditLog {
    Table,
    Id,
    UserId,
    TokenPrefix,
    ClientIp,
    Tool,
    Project,
    Detail,
    Ok,
    Error,
    CreatedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ApiTokens::Table)
                    .add_column(ColumnDef::new(ApiTokens::ExpiresAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(McpAuditLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(McpAuditLog::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(McpAuditLog::UserId).uuid())
                    .col(ColumnDef::new(McpAuditLog::TokenPrefix).string_len(32))
                    .col(ColumnDef::new(McpAuditLog::ClientIp).string_len(64))
                    .col(ColumnDef::new(McpAuditLog::Tool).string_len(64).not_null())
                    .col(ColumnDef::new(McpAuditLog::Project).string_len(128))
                    .col(ColumnDef::new(McpAuditLog::Detail).string_len(320))
                    .col(
                        ColumnDef::new(McpAuditLog::Ok)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(ColumnDef::new(McpAuditLog::Error).string_len(320))
                    .col(
                        ColumnDef::new(McpAuditLog::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-mcp-audit-created")
                    .table(McpAuditLog::Table)
                    .col(McpAuditLog::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(McpAuditLog::Table).to_owned())
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(ApiTokens::Table)
                    .drop_column(ApiTokens::ExpiresAt)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
