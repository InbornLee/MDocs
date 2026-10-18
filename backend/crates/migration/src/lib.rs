pub use sea_orm_migration::prelude::*;

mod m20260914_000001_init;
mod m20260914_000002_publish;
mod m20260914_000003_api_token;
mod m20260915_000001_admin;
mod m20261012_000001_ai_kb_p0;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260914_000001_init::Migration),
            Box::new(m20260914_000002_publish::Migration),
            Box::new(m20260914_000003_api_token::Migration),
            Box::new(m20260915_000001_admin::Migration),
            Box::new(m20261012_000001_ai_kb_p0::Migration),
        ]
    }
}
