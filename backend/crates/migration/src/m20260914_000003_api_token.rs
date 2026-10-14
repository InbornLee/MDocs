use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum ApiTokens {
    Table,
    Id,
    UserId,
    Name,
    TokenHash,
    TokenPrefix,
    CreatedAt,
    LastUsedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ApiTokens::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ApiTokens::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(ApiTokens::UserId).uuid().not_null())
                    .col(ColumnDef::new(ApiTokens::Name).string_len(64).not_null())
                    // 只存 sha256 十六进制，明文仅创建时返回一次
                    .col(ColumnDef::new(ApiTokens::TokenHash).string_len(64).not_null())
                    // 供页面辨认的前缀
                    .col(ColumnDef::new(ApiTokens::TokenPrefix).string_len(32).not_null())
                    .col(
                        ColumnDef::new(ApiTokens::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ApiTokens::LastUsedAt).timestamp_with_time_zone())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-api-tokens-user")
                            .from(ApiTokens::Table, ApiTokens::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-api-tokens-hash")
                    .unique()
                    .table(ApiTokens::Table)
                    .col(ApiTokens::TokenHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-api-tokens-user")
                    .table(ApiTokens::Table)
                    .col(ApiTokens::UserId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ApiTokens::Table).to_owned())
            .await?;
        Ok(())
    }
}
