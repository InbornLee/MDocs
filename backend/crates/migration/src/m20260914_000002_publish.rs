use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum Documents {
    Table,
    Published,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Documents::Table)
                    .add_column(
                        ColumnDef::new(Documents::Published)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-documents-published")
                    .table(Documents::Table)
                    .col(Documents::Published)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Documents::Table)
                    .drop_column(Documents::Published)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
