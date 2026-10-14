import { ref } from 'vue'

const TOKEN_KEY = 'mdocs_token'

// 会话 token 放在 ref 里而不是直接读 localStorage：顶栏的「登录/注册」与用户头像、
// 公开页的入口按钮都看它，非响应式的话登录 / 退出后这些地方不会刷新。
// 所有写入都走 setToken，401 时 client 自己清 token 的那条路径也能同步到界面。
const token = ref<string | null>(localStorage.getItem(TOKEN_KEY))

export function getToken(): string | null {
  return token.value
}

export function setToken(next: string | null) {
  token.value = next
  if (next) localStorage.setItem(TOKEN_KEY, next)
  else localStorage.removeItem(TOKEN_KEY)
}

export class ApiError extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

async function request<T>(method: string, url: string, body?: unknown, raw?: boolean): Promise<T> {
  const headers: Record<string, string> = {}
  const bearer = getToken()
  if (bearer) headers['Authorization'] = `Bearer ${bearer}`
  if (body !== undefined && !(body instanceof FormData)) {
    headers['Content-Type'] = 'application/json'
  }
  const resp = await fetch(url, {
    method,
    headers,
    body: body === undefined ? undefined : body instanceof FormData ? body : JSON.stringify(body),
  })
  if (!resp.ok) {
    let msg = `HTTP ${resp.status}`
    try {
      const data = await resp.json()
      if (data?.error) msg = data.error
    } catch {
      /* ignore */
    }
    if (resp.status === 401) {
      setToken(null)
      if (!location.pathname.startsWith('/login') && !location.pathname.startsWith('/register')) {
        location.href = '/login'
      }
    }
    throw new ApiError(resp.status, msg)
  }
  if (raw) return resp as unknown as T
  const ct = resp.headers.get('content-type') || ''
  if (ct.includes('application/json')) return (await resp.json()) as T
  return (await resp.text()) as unknown as T
}

export const api = {
  get: <T>(url: string) => request<T>('GET', url),
  getRaw: (url: string) => request<Response>('GET', url, undefined, true),
  post: <T>(url: string, body?: unknown) => request<T>('POST', url, body),
  put: <T>(url: string, body?: unknown) => request<T>('PUT', url, body),
  patch: <T>(url: string, body?: unknown) => request<T>('PATCH', url, body),
  del: <T>(url: string) => request<T>('DELETE', url),
  upload: <T>(url: string, form: FormData) => request<T>('POST', url, form),
}
