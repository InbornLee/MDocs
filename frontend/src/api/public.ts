import { api } from './client'
import type { LinkEdge, LinkNode, SearchResultDto, SiteSettingsDto } from './types'

/** 站点级公开配置：首页标题由管理员在管理界面里配置 */
export function getSiteSettings() {
  return api.get<SiteSettingsDto>('/api/public/site')
}

export interface PublicProjectItem {
  id: string
  slug: string
  name: string
  description: string | null
  owner_name: string
  doc_count: number
  updated_at: string
}

export interface PublicProjectList {
  items: PublicProjectItem[]
  total: number
  page: number
  page_size: number
}

export interface PublicBranch {
  id: string
  name: string
  is_default: boolean
}

export interface PublicDocItem {
  id: string
  path: string
  title: string
  size_bytes: number
  updated_at: string
}

export interface LinkSummary {
  nodes: number
  edges: number
  dangling: number
}

export interface PublicProjectDetail {
  id: string
  slug: string
  name: string
  description: string | null
  owner_name: string
  created_at: string
  updated_at: string
  branch: PublicBranch
  branches: PublicBranch[]
  docs: PublicDocItem[]
  link_summary: LinkSummary
}

export interface PublicDocContent {
  id: string
  path: string
  title: string
  content: string
  version_id: string
  updated_at: string
  size_bytes: number
}

export interface PublicLinksDto {
  branch: PublicBranch
  nodes: LinkNode[]
  edges: LinkEdge[]
}

export function listPublicProjects(params: {
  query?: string
  page?: number
  pageSize?: number
} = {}) {
  const qs = new URLSearchParams()
  if (params.query) qs.set('query', params.query)
  if (params.page) qs.set('page', String(params.page))
  if (params.pageSize) qs.set('page_size', String(params.pageSize))
  const q = qs.toString() ? `?${qs.toString()}` : ''
  return api.get<PublicProjectList>(`/api/public/projects${q}`)
}

export function getPublicProject(slug: string, branch?: string) {
  const q = branch ? `?branch=${encodeURIComponent(branch)}` : ''
  return api.get<PublicProjectDetail>(`/api/public/projects/${slug}${q}`)
}

export function getPublicDocContent(slug: string, docId: string, branch?: string) {
  const q = branch ? `?branch=${encodeURIComponent(branch)}` : ''
  return api.get<PublicDocContent>(
    `/api/public/projects/${slug}/docs/${docId}/content${q}`,
  )
}

export function getPublicLinks(slug: string, branch?: string) {
  const q = branch ? `?branch=${encodeURIComponent(branch)}` : ''
  return api.get<PublicLinksDto>(`/api/public/projects/${slug}/links${q}`)
}

/** 公开站点的全文检索：只覆盖已发布文档 */
export function searchPublicDocs(
  slug: string,
  params: { q: string; branch?: string; limit?: number },
) {
  const qs = new URLSearchParams({ q: params.q })
  if (params.branch) qs.set('branch', params.branch)
  if (params.limit) qs.set('limit', String(params.limit))
  return api.get<SearchResultDto>(`/api/public/projects/${slug}/search?${qs.toString()}`)
}
