import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { api, getToken, setToken } from '@/api/client'

export interface UserDto {
  id: string
  username: string
  email: string
  display_name: string
  avatar_file: string | null
  bio: string | null
  /** 内置 admin 账号；前端据此显示「管理」入口 */
  is_admin: boolean
  created_at: string
}

export const useAuthStore = defineStore('auth', () => {
  const user = ref<UserDto | null>(null)
  const loaded = ref(false)

  const isLoggedIn = computed(() => !!getToken())
  const isAdmin = computed(() => user.value?.is_admin === true)
  const displayName = computed(() => {
    if (!user.value) return ''
    return user.value.display_name || user.value.username
  })
  const avatarUrl = computed(() =>
    user.value?.avatar_file ? `/uploads/avatars/${user.value.avatar_file}` : null,
  )

  async function fetchMe() {
    if (!getToken()) {
      user.value = null
      loaded.value = true
      return
    }
    try {
      user.value = await api.get<UserDto>('/api/auth/me')
    } catch {
      user.value = null
    }
    loaded.value = true
  }

  function setSession(token: string, u: UserDto) {
    setToken(token)
    user.value = u
  }

  function logout() {
    setToken(null)
    user.value = null
  }

  return { user, loaded, isLoggedIn, isAdmin, displayName, avatarUrl, fetchMe, setSession, logout }
})
