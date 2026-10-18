export interface ProjectDto {
  id: string
  slug: string
  name: string
  description: string | null
  visibility: 'private' | 'public'
  role: string
  /** 是否持有 owner 角色（被授予 owner 的协作者也算） */
  is_owner: boolean
  /** 是否是项目创建者：非创建者即协作者，可以退出项目 */
  is_creator: boolean
  default_branch: string | null
  created_at: string
  updated_at: string
}

export interface BranchDto {
  id: string
  name: string
  head_commit_id: string | null
  is_default: boolean
  created_at: string
}

export interface DocListItem {
  id: string
  path: string
  title: string
  published: boolean
  version_id: string | null
  updated_at: string
  size_bytes: number
}

export interface DocsDto {
  branch: { id: string; name: string; is_default: boolean }
  docs: DocListItem[]
}

export interface DocContent {
  id: string
  path: string
  title: string
  content: string
  version_id: string
  commit_id: string
  size_bytes: number
  updated_at: string
  tags: string[]
  description: string | null
}

export interface SaveResult {
  id: string
  version_id?: string
  commit_id?: string
  updated_at?: string
  unchanged?: boolean
}

export interface LinkNode {
  id: string
  path: string
  title: string
  out: number
  inn: number
  dangling: number
}

export interface LinkEdge {
  source: string
  target: string | null
  raw_target: string
  type: 'md_relative' | 'wiki'
  anchor: string | null
}

export interface LinksDto {
  branch: { id: string; name: string; is_default: boolean }
  nodes: LinkNode[]
  edges: LinkEdge[]
}

export interface MrListItem {
  id: string
  title: string
  status: 'open' | 'merged' | 'closed'
  source: string
  target: string
  target_is_default: boolean
  conflict_files: string[]
  created_by: string
  created_at: string
  merged_at: string | null
}

export interface MrFilePlan {
  document_id: string
  path: string
  title: string
  change: 'added' | 'modified' | 'deleted'
  has_conflict: boolean
  conflict_kind: 'content' | 'add-add' | 'modify-delete' | null
  base_content: string | null
  ours_content: string | null
  theirs_content: string | null
  merged_content: string | null
}

export interface MrReview {
  id: string
  reviewer_id: string
  reviewer: string
  action: 'approve' | 'reject' | 'comment'
  comment: string | null
  created_at: string
}

export interface MrDetail {
  id: string
  title: string
  description: string | null
  status: 'open' | 'merged' | 'closed'
  source: string
  source_id: string
  target: string
  target_id: string
  target_is_default: boolean
  base_commit_id: string | null
  merge_commit_id: string | null
  created_by: string
  created_by_id: string
  created_at: string
  merged_at: string | null
  merge_base: string | null
  plan: MrFilePlan[]
  reviews: MrReview[]
  my_role: string
  can_merge: boolean
  conflicts: string[]
  blocked_reason: string | null
}

export interface MemberDto {
  user_id: string
  username: string
  display_name: string
  avatar_file: string | null
  role: 'owner' | 'editor' | 'reader'
  is_owner: boolean
  created_at: string
}

/** 「添加成员」下拉框的候选用户 */
export interface MemberCandidateDto {
  username: string
  display_name: string
  avatar_file: string | null
}

/** 管理界面：站点配置 */
export interface SiteSettingsDto {
  /** 首页大标题，未配置时后端返回默认值「公开项目」 */
  home_title: string
  /** 关于页展示的仓库地址 / 反馈入口，未配置为 null */
  repo_url: string | null
  feedback_url: string | null
  feedback_email: string | null
}

/** 管理界面：注册成员 */
export interface AdminUserDto {
  id: string
  username: string
  email: string
  display_name: string
  enabled: boolean
  is_admin: boolean
  project_count: number
  created_at: string
}

export interface AdminUserListDto {
  items: AdminUserDto[]
  total: number
  page: number
  page_size: number
}

/** 管理界面：公开项目 */
export interface AdminProjectDto {
  id: string
  slug: string
  name: string
  description: string | null
  owner_name: string
  updated_at: string
}

export interface AdminProjectListDto {
  items: AdminProjectDto[]
  total: number
  page: number
  page_size: number
}

export interface RevisionDto {
  commit_id: string
  message: string
  commit_type: string
  author: string
  created_at: string
  change_type: 'added' | 'modified' | 'deleted' | 'renamed'
  version_id: string | null
  size_bytes: number
  current: boolean
}

export interface SearchHitDto {
  doc_id: string
  path: string
  title: string
  /** 命中所在行号（从 1 开始） */
  line: number
  /** 该文档内的第几处命中（从 1 开始），用于在正文里定位高亮 */
  nth: number
  before: string
  matched: string
  after: string
}

export interface SearchResultDto {
  branch: { id: string; name: string; is_default: boolean }
  query: string
  /** 全部命中数，可能大于返回的条数 */
  total: number
  truncated: boolean
  hits: SearchHitDto[]
}

export interface DiffDto {
  branch: string
  from_commit: string | null
  to_commit: string | null
  from_label: string
  to_label: string
  has_changes: boolean
  diff: string
  old_content: string
  new_content: string
}

export interface RevertResult {
  id: string
  version_id?: string
  commit_id?: string
  restored_from?: string
  branch?: string
  unchanged?: boolean
}
