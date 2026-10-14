import { api } from './client'
import type {
  BranchDto,
  DiffDto,
  DocContent,
  DocsDto,
  LinksDto,
  MemberCandidateDto,
  MemberDto,
  MrDetail,
  MrListItem,
  ProjectDto,
  RevertResult,
  RevisionDto,
  SaveResult,
  SearchResultDto,
} from './types'

// ---------- projects ----------

export function listProjects() {
  return api.get<ProjectDto[]>('/api/projects')
}

export function createProject(body: { name: string; slug?: string; description?: string; visibility?: string }) {
  return api.post<ProjectDto>('/api/projects', body)
}

export function getProject(slug: string) {
  return api.get<{ project: ProjectDto; branches: BranchDto[]; commit_count: number }>(
    `/api/projects/${slug}`,
  )
}

export function updateProject(slug: string, body: { name?: string; description?: string; visibility?: string }) {
  return api.patch<ProjectDto>(`/api/projects/${slug}`, body)
}

export function deleteProject(slug: string) {
  return api.del<{ deleted: string }>(`/api/projects/${slug}`)
}

// ---------- branches ----------

export function listBranches(slug: string) {
  return api.get<BranchDto[]>(`/api/projects/${slug}/branches`)
}

export function createBranch(slug: string, body: { name: string; from_branch?: string }) {
  return api.post<BranchDto>(`/api/projects/${slug}/branches`, body)
}

// ---------- documents ----------

export function listDocs(slug: string, branch?: string) {
  const q = branch ? `?branch=${encodeURIComponent(branch)}` : ''
  return api.get<DocsDto>(`/api/projects/${slug}/docs${q}`)
}

export function getDocContent(slug: string, docId: string, branch?: string) {
  const q = branch ? `?branch=${encodeURIComponent(branch)}` : ''
  return api.get<DocContent>(`/api/projects/${slug}/docs/${docId}/content${q}`)
}

export function createDoc(
  slug: string,
  body: { path: string; content: string; branch?: string; message?: string },
) {
  return api.post<{ id: string; path: string; title: string; version_id: string }>(
    `/api/projects/${slug}/docs`,
    body,
  )
}

export function saveDoc(
  slug: string,
  docId: string,
  body: { content: string; message?: string; branch?: string; base_version_id?: string },
) {
  return api.put<SaveResult>(`/api/projects/${slug}/docs/${docId}`, body)
}

export function moveDoc(slug: string, docId: string, body: { new_path: string; branch?: string }) {
  return api.patch<{ id: string; path: string }>(`/api/projects/${slug}/docs/${docId}`, body)
}

export function deleteDoc(slug: string, docId: string, branch?: string) {
  const q = branch ? `?branch=${encodeURIComponent(branch)}` : ''
  return api.del<{ id: string; commit_id: string }>(`/api/projects/${slug}/docs/${docId}${q}`)
}

// ---------- link graph ----------

export function getLinks(slug: string, branch?: string) {
  const q = branch ? `?branch=${encodeURIComponent(branch)}` : ''
  return api.get<LinksDto>(`/api/projects/${slug}/links${q}`)
}

// ---------- version history ----------

export function getDocHistory(slug: string, docId: string, branch?: string) {
  const q = branch ? `?branch=${encodeURIComponent(branch)}` : ''
  return api.get<RevisionDto[]>(`/api/projects/${slug}/docs/${docId}/history${q}`)
}

export function getDocDiff(
  slug: string,
  docId: string,
  params: { branch?: string; from?: string | null; to?: string | null; context?: number } = {},
) {
  const qs = new URLSearchParams()
  if (params.branch) qs.set('branch', params.branch)
  if (params.from) qs.set('from', params.from)
  if (params.to) qs.set('to', params.to)
  if (params.context != null) qs.set('context', String(params.context))
  const q = qs.toString() ? `?${qs.toString()}` : ''
  return api.get<DiffDto>(`/api/projects/${slug}/docs/${docId}/diff${q}`)
}

export function revertDoc(
  slug: string,
  docId: string,
  body: { commit_id: string; branch?: string; message?: string },
) {
  return api.post<RevertResult>(`/api/projects/${slug}/docs/${docId}/revert`, body)
}

// ---------- 全文检索 ----------

export function searchDocs(
  slug: string,
  params: { q: string; branch?: string; limit?: number },
) {
  const qs = new URLSearchParams({ q: params.q })
  if (params.branch) qs.set('branch', params.branch)
  if (params.limit) qs.set('limit', String(params.limit))
  return api.get<SearchResultDto>(`/api/projects/${slug}/search?${qs.toString()}`)
}

export function setDocPublished(slug: string, docId: string, published: boolean) {
  return api.patch<{ id: string; published: boolean; project_public: boolean }>(
    `/api/projects/${slug}/docs/${docId}/publish`,
    { published },
  )
}

// ---------- merge requests ----------

export function listMrs(slug: string, status?: string) {
  const q = status && status !== 'all' ? `?status=${encodeURIComponent(status)}` : ''
  return api.get<MrListItem[]>(`/api/projects/${slug}/mrs${q}`)
}

export function createMr(
  slug: string,
  body: { source: string; target?: string; title: string; description?: string },
) {
  return api.post<{ id: string; status: string; conflicts: string[]; changed_files: number }>(
    `/api/projects/${slug}/mrs`,
    body,
  )
}

export function getMr(slug: string, mrId: string) {
  return api.get<MrDetail>(`/api/projects/${slug}/mrs/${mrId}`)
}

export function addMrReview(
  slug: string,
  mrId: string,
  body: { action: 'approve' | 'reject' | 'comment'; comment?: string },
) {
  return api.post<{ id: string; action: string }>(`/api/projects/${slug}/mrs/${mrId}/reviews`, body)
}

export function mergeMr(
  slug: string,
  mrId: string,
  body: {
    resolutions?: Record<string, string | null>
    deletes?: string[]
    message?: string
  },
) {
  return api.post<{
    id: string
    status: string
    merge_commit_id: string
    files: number
    conflicts_resolved: number
  }>(`/api/projects/${slug}/mrs/${mrId}/merge`, body)
}

export function closeMr(slug: string, mrId: string) {
  return api.post<{ id: string; status: string }>(`/api/projects/${slug}/mrs/${mrId}/close`)
}

// ---------- members ----------

export function listMembers(slug: string) {
  return api.get<MemberDto[]>(`/api/projects/${slug}/members`)
}

export function addMember(slug: string, body: { user: string; role?: string }) {
  return api.post<MemberDto>(`/api/projects/${slug}/members`, body)
}

/** 添加成员时按用户名 / 昵称 / 邮箱搜索候选用户 */
export function searchMemberCandidates(slug: string, q: string, limit = 10) {
  const qs = new URLSearchParams({ q, limit: String(limit) })
  return api.get<MemberCandidateDto[]>(
    `/api/projects/${slug}/member-candidates?${qs.toString()}`,
  )
}

export function updateMember(slug: string, userId: string, role: string) {
  return api.patch<{ user_id: string; role: string }>(
    `/api/projects/${slug}/members/${userId}`,
    { role },
  )
}

export function removeMember(slug: string, userId: string) {
  return api.del<{ removed: string }>(`/api/projects/${slug}/members/${userId}`)
}

/** 退出项目：移除的是服务端从会话里取到的自己，前端不需要传 user id */
export function leaveProject(slug: string) {
  return api.del<{ removed: string }>(`/api/projects/${slug}/members/me`)
}
