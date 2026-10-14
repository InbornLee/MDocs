use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum Users {
    Table,
    Enabled,
}

#[derive(DeriveIden)]
enum Settings {
    Table,
    Key,
    Value,
    UpdatedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 被管理员禁用的账号不能登录，已签发的 token 也会被中间件拒掉
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(
                        ColumnDef::new(Users::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        // 站点级配置（目前只有首页标题），key-value 一条一项
        manager
            .create_table(
                Table::create()
                    .table(Settings::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Settings::Key)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Settings::Value).text().not_null())
                    .col(
                        ColumnDef::new(Settings::UpdatedAt)
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
            .drop_table(Table::drop().table(Settings::Table).to_owned())
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .drop_column(Users::Enabled)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
