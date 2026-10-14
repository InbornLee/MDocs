use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Username,
    Email,
    PasswordHash,
    DisplayName,
    AvatarFile,
    Bio,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    OwnerId,
    Slug,
    Name,
    Description,
    Visibility,
    DefaultBranchId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ProjectMembers {
    Table,
    Id,
    ProjectId,
    UserId,
    Role,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Branches {
    Table,
    Id,
    ProjectId,
    Name,
    HeadCommitId,
    CreatedBy,
    IsDefault,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Commits {
    Table,
    Id,
    ProjectId,
    BranchId,
    ParentId,
    AuthorId,
    Message,
    CommitType,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Documents {
    Table,
    Id,
    ProjectId,
    Path,
    Title,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum DocVersions {
    Table,
    Id,
    DocumentId,
    Content,
    ContentHash,
    SizeBytes,
    CreatedBy,
    CreatedAt,
}

#[derive(DeriveIden)]
enum CommitDocs {
    Table,
    Id,
    CommitId,
    DocumentId,
    ChangeType,
    VersionId,
    OldPath,
}

#[derive(DeriveIden)]
enum LinkEdges {
    Table,
    Id,
    ProjectId,
    BranchId,
    SourceDocId,
    TargetDocId,
    RawTarget,
    LinkType,
    Anchor,
    DisplayText,
    LineNo,
}

#[derive(DeriveIden)]
enum MergeRequests {
    Table,
    Id,
    ProjectId,
    SourceBranchId,
    TargetBranchId,
    BaseCommitId,
    Title,
    Description,
    Status,
    ConflictFiles,
    MergeCommitId,
    CreatedBy,
    ReviewedBy,
    MergedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum MrReviews {
    Table,
    Id,
    MrId,
    ReviewerId,
    Action,
    Comment,
    CreatedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ---------- users ----------
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Users::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Users::Username).string_len(64).not_null())
                    .col(ColumnDef::new(Users::Email).string_len(255).not_null())
                    .col(ColumnDef::new(Users::PasswordHash).string_len(255).not_null())
                    .col(ColumnDef::new(Users::DisplayName).string_len(128).not_null().default(""))
                    .col(ColumnDef::new(Users::AvatarFile).string_len(255))
                    .col(ColumnDef::new(Users::Bio).text())
                    .col(ColumnDef::new(Users::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Users::UpdatedAt).timestamp_with_time_zone().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-users-username")
                    .unique()
                    .table(Users::Table)
                    .col(Users::Username)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-users-email")
                    .unique()
                    .table(Users::Table)
                    .col(Users::Email)
                    .to_owned(),
            )
            .await?;

        // ---------- projects ----------
        manager
            .create_table(
                Table::create()
                    .table(Projects::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Projects::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Projects::OwnerId).uuid().not_null())
                    .col(ColumnDef::new(Projects::Slug).string_len(64).not_null())
                    .col(ColumnDef::new(Projects::Name).string_len(128).not_null())
                    .col(ColumnDef::new(Projects::Description).text())
                    .col(ColumnDef::new(Projects::Visibility).string_len(16).not_null().default("private"))
                    .col(ColumnDef::new(Projects::DefaultBranchId).uuid())
                    .col(ColumnDef::new(Projects::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Projects::UpdatedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-projects-owner")
                            .from(Projects::Table, Projects::OwnerId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-projects-slug")
                    .unique()
                    .table(Projects::Table)
                    .col(Projects::Slug)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-projects-owner")
                    .table(Projects::Table)
                    .col(Projects::OwnerId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-projects-visibility")
                    .table(Projects::Table)
                    .col(Projects::Visibility)
                    .to_owned(),
            )
            .await?;

        // ---------- project_members ----------
        manager
            .create_table(
                Table::create()
                    .table(ProjectMembers::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ProjectMembers::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(ProjectMembers::ProjectId).uuid().not_null())
                    .col(ColumnDef::new(ProjectMembers::UserId).uuid().not_null())
                    .col(ColumnDef::new(ProjectMembers::Role).string_len(16).not_null())
                    .col(ColumnDef::new(ProjectMembers::CreatedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-members-project")
                            .from(ProjectMembers::Table, ProjectMembers::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-members-user")
                            .from(ProjectMembers::Table, ProjectMembers::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-members-project-user")
                    .unique()
                    .table(ProjectMembers::Table)
                    .col(ProjectMembers::ProjectId)
                    .col(ProjectMembers::UserId)
                    .to_owned(),
            )
            .await?;

        // ---------- branches ----------
        manager
            .create_table(
                Table::create()
                    .table(Branches::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Branches::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Branches::ProjectId).uuid().not_null())
                    .col(ColumnDef::new(Branches::Name).string_len(128).not_null())
                    .col(ColumnDef::new(Branches::HeadCommitId).uuid())
                    .col(ColumnDef::new(Branches::CreatedBy).uuid().not_null())
                    .col(ColumnDef::new(Branches::IsDefault).boolean().not_null().default(false))
                    .col(ColumnDef::new(Branches::CreatedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-branches-project")
                            .from(Branches::Table, Branches::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-branches-project-name")
                    .unique()
                    .table(Branches::Table)
                    .col(Branches::ProjectId)
                    .col(Branches::Name)
                    .to_owned(),
            )
            .await?;

        // ---------- commits ----------
        manager
            .create_table(
                Table::create()
                    .table(Commits::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Commits::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Commits::ProjectId).uuid().not_null())
                    .col(ColumnDef::new(Commits::BranchId).uuid().not_null())
                    .col(ColumnDef::new(Commits::ParentId).uuid())
                    .col(ColumnDef::new(Commits::AuthorId).uuid().not_null())
                    .col(ColumnDef::new(Commits::Message).string_len(500).not_null())
                    .col(ColumnDef::new(Commits::CommitType).string_len(16).not_null().default("edit"))
                    .col(ColumnDef::new(Commits::CreatedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-commits-project")
                            .from(Commits::Table, Commits::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-commits-branch")
                            .from(Commits::Table, Commits::BranchId)
                            .to(Branches::Table, Branches::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-commits-author")
                            .from(Commits::Table, Commits::AuthorId)
                            .to(Users::Table, Users::Id),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-commits-branch-created")
                    .table(Commits::Table)
                    .col(Commits::BranchId)
                    .col(Commits::CreatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-commits-parent")
                    .table(Commits::Table)
                    .col(Commits::ParentId)
                    .to_owned(),
            )
            .await?;

        // ---------- documents ----------
        manager
            .create_table(
                Table::create()
                    .table(Documents::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Documents::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Documents::ProjectId).uuid().not_null())
                    .col(ColumnDef::new(Documents::Path).string_len(1024).not_null())
                    .col(ColumnDef::new(Documents::Title).string_len(255).not_null())
                    .col(ColumnDef::new(Documents::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Documents::UpdatedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-documents-project")
                            .from(Documents::Table, Documents::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-documents-project-path")
                    .unique()
                    .table(Documents::Table)
                    .col(Documents::ProjectId)
                    .col(Documents::Path)
                    .to_owned(),
            )
            .await?;

        // ---------- doc_versions ----------
        manager
            .create_table(
                Table::create()
                    .table(DocVersions::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(DocVersions::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(DocVersions::DocumentId).uuid().not_null())
                    .col(ColumnDef::new(DocVersions::Content).text().not_null())
                    .col(ColumnDef::new(DocVersions::ContentHash).string_len(64).not_null())
                    .col(ColumnDef::new(DocVersions::SizeBytes).integer().not_null())
                    .col(ColumnDef::new(DocVersions::CreatedBy).uuid().not_null())
                    .col(ColumnDef::new(DocVersions::CreatedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-versions-document")
                            .from(DocVersions::Table, DocVersions::DocumentId)
                            .to(Documents::Table, Documents::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-versions-doc-created")
                    .table(DocVersions::Table)
                    .col(DocVersions::DocumentId)
                    .col(DocVersions::CreatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-versions-doc-hash")
                    .table(DocVersions::Table)
                    .col(DocVersions::DocumentId)
                    .col(DocVersions::ContentHash)
                    .to_owned(),
            )
            .await?;

        // ---------- commit_docs ----------
        manager
            .create_table(
                Table::create()
                    .table(CommitDocs::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(CommitDocs::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(CommitDocs::CommitId).uuid().not_null())
                    .col(ColumnDef::new(CommitDocs::DocumentId).uuid().not_null())
                    .col(ColumnDef::new(CommitDocs::ChangeType).string_len(16).not_null())
                    .col(ColumnDef::new(CommitDocs::VersionId).uuid())
                    .col(ColumnDef::new(CommitDocs::OldPath).string_len(1024))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-commitdocs-commit")
                            .from(CommitDocs::Table, CommitDocs::CommitId)
                            .to(Commits::Table, Commits::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-commitdocs-document")
                            .from(CommitDocs::Table, CommitDocs::DocumentId)
                            .to(Documents::Table, Documents::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-commitdocs-commit-doc")
                    .unique()
                    .table(CommitDocs::Table)
                    .col(CommitDocs::CommitId)
                    .col(CommitDocs::DocumentId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-commitdocs-document")
                    .table(CommitDocs::Table)
                    .col(CommitDocs::DocumentId)
                    .to_owned(),
            )
            .await?;

        // ---------- link_edges ----------
        manager
            .create_table(
                Table::create()
                    .table(LinkEdges::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(LinkEdges::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(LinkEdges::ProjectId).uuid().not_null())
                    .col(ColumnDef::new(LinkEdges::BranchId).uuid().not_null())
                    .col(ColumnDef::new(LinkEdges::SourceDocId).uuid().not_null())
                    .col(ColumnDef::new(LinkEdges::TargetDocId).uuid())
                    .col(ColumnDef::new(LinkEdges::RawTarget).string_len(1024).not_null())
                    .col(ColumnDef::new(LinkEdges::LinkType).string_len(16).not_null())
                    .col(ColumnDef::new(LinkEdges::Anchor).string_len(255))
                    .col(ColumnDef::new(LinkEdges::DisplayText).string_len(255))
                    .col(ColumnDef::new(LinkEdges::LineNo).integer())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-edges-project")
                            .from(LinkEdges::Table, LinkEdges::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-edges-branch")
                            .from(LinkEdges::Table, LinkEdges::BranchId)
                            .to(Branches::Table, Branches::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-edges-source")
                            .from(LinkEdges::Table, LinkEdges::SourceDocId)
                            .to(Documents::Table, Documents::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-edges-target")
                            .from(LinkEdges::Table, LinkEdges::TargetDocId)
                            .to(Documents::Table, Documents::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-edges-branch-src-raw-line")
                    .unique()
                    .table(LinkEdges::Table)
                    .col(LinkEdges::BranchId)
                    .col(LinkEdges::SourceDocId)
                    .col(LinkEdges::RawTarget)
                    .col(LinkEdges::LineNo)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-edges-branch-source")
                    .table(LinkEdges::Table)
                    .col(LinkEdges::ProjectId)
                    .col(LinkEdges::BranchId)
                    .col(LinkEdges::SourceDocId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-edges-branch-target")
                    .table(LinkEdges::Table)
                    .col(LinkEdges::ProjectId)
                    .col(LinkEdges::BranchId)
                    .col(LinkEdges::TargetDocId)
                    .to_owned(),
            )
            .await?;

        // ---------- merge_requests ----------
        manager
            .create_table(
                Table::create()
                    .table(MergeRequests::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(MergeRequests::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(MergeRequests::ProjectId).uuid().not_null())
                    .col(ColumnDef::new(MergeRequests::SourceBranchId).uuid().not_null())
                    .col(ColumnDef::new(MergeRequests::TargetBranchId).uuid().not_null())
                    .col(ColumnDef::new(MergeRequests::BaseCommitId).uuid().not_null())
                    .col(ColumnDef::new(MergeRequests::Title).string_len(255).not_null())
                    .col(ColumnDef::new(MergeRequests::Description).text())
                    .col(ColumnDef::new(MergeRequests::Status).string_len(16).not_null().default("open"))
                    .col(ColumnDef::new(MergeRequests::ConflictFiles).text())
                    .col(ColumnDef::new(MergeRequests::MergeCommitId).uuid())
                    .col(ColumnDef::new(MergeRequests::CreatedBy).uuid().not_null())
                    .col(ColumnDef::new(MergeRequests::ReviewedBy).uuid())
                    .col(ColumnDef::new(MergeRequests::MergedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(MergeRequests::CreatedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-mrs-project")
                            .from(MergeRequests::Table, MergeRequests::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-mrs-source")
                            .from(MergeRequests::Table, MergeRequests::SourceBranchId)
                            .to(Branches::Table, Branches::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-mrs-target")
                            .from(MergeRequests::Table, MergeRequests::TargetBranchId)
                            .to(Branches::Table, Branches::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-mrs-project-status")
                    .table(MergeRequests::Table)
                    .col(MergeRequests::ProjectId)
                    .col(MergeRequests::Status)
                    .to_owned(),
            )
            .await?;

        // ---------- mr_reviews ----------
        manager
            .create_table(
                Table::create()
                    .table(MrReviews::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(MrReviews::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(MrReviews::MrId).uuid().not_null())
                    .col(ColumnDef::new(MrReviews::ReviewerId).uuid().not_null())
                    .col(ColumnDef::new(MrReviews::Action).string_len(24).not_null())
                    .col(ColumnDef::new(MrReviews::Comment).text())
                    .col(ColumnDef::new(MrReviews::CreatedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-reviews-mr")
                            .from(MrReviews::Table, MrReviews::MrId)
                            .to(MergeRequests::Table, MergeRequests::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-reviews-reviewer")
                            .from(MrReviews::Table, MrReviews::ReviewerId)
                            .to(Users::Table, Users::Id),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-reviews-mr")
                    .table(MrReviews::Table)
                    .col(MrReviews::MrId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let tables: Vec<DynIden> = vec![
            MrReviews::Table.into_iden(),
            MergeRequests::Table.into_iden(),
            LinkEdges::Table.into_iden(),
            CommitDocs::Table.into_iden(),
            DocVersions::Table.into_iden(),
            Documents::Table.into_iden(),
            Commits::Table.into_iden(),
            Branches::Table.into_iden(),
            ProjectMembers::Table.into_iden(),
            Projects::Table.into_iden(),
            Users::Table.into_iden(),
        ];
        for t in tables {
            manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
        }
        Ok(())
    }
}
