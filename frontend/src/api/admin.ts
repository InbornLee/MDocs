import { api } from './client'
import type {
  AdminProjectListDto,
  AdminUserListDto,
  SiteSettingsDto,
} from './types'

/** 注册成员列表（管理员） */
export function listAdminUsers(params: { query?: string; page?: number; pageSize?: number } = {}) {
  const qs = new URLSearchParams()
  if (params.query) qs.set('query', params.query)
  if (params.page) qs.set('page', String(params.page))
  if (params.pageSize) qs.set('page_size', String(params.pageSize))
  const q = qs.toString() ? `?${qs.toString()}` : ''
  return api.get<AdminUserListDto>(`/api/admin/users${q}`)
}

/** 禁用 / 启用某个注册账号 */
export function setUserEnabled(id: string, enabled: boolean) {
  return api.patch<{ id: string; enabled: boolean }>(`/api/admin/users/${id}`, { enabled })
}

/** 重置某个注册成员的密码，返回的新密码只显示这一次 */
export function resetUserPassword(id: string) {
  return api.post<{ id: string; username: string; password: string }>(
    `/api/admin/users/${id}/reset-password`,
  )
}

/** 公开项目列表（管理员） */
export function listAdminProjects(
  params: { query?: string; page?: number; pageSize?: number } = {},
) {
  const qs = new URLSearchParams()
  if (params.query) qs.set('query', params.query)
  if (params.page) qs.set('page', String(params.page))
  if (params.pageSize) qs.set('page_size', String(params.pageSize))
  const q = qs.toString() ? `?${qs.toString()}` : ''
  return api.get<AdminProjectListDto>(`/api/admin/projects${q}`)
}

/** 取消公开：把项目改回私有 */
export function unpublishProject(slug: string) {
  return api.patch<{ slug: string; visibility: string }>(`/api/admin/projects/${slug}`, {
    visibility: 'private',
  })
}

/** 保存站点配置（首页标题 + 关于页的仓库/反馈入口，留空即清掉） */
export function updateSiteSettings(body: {
  home_title: string
  repo_url: string
  feedback_url: string
  feedback_email: string
}) {
  return api.put<SiteSettingsDto>('/api/admin/site', body)
}
