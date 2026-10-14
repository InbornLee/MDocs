<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  NConfigProvider,
  NGlobalStyle,
  NMessageProvider,
  NDialogProvider,
  NButton,
  NAvatar,
  NDropdown,
  NIcon,
  darkTheme,
  zhCN,
  dateZhCN,
} from 'naive-ui'
import { MoonOutline, SunnyOutline } from '@vicons/ionicons5'
import { useAuthStore } from '@/stores/auth'
import { useThemeStore } from '@/stores/theme'
import { darkThemeOverrides, lightThemeOverrides } from '@/theme'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const theme = useThemeStore()

const naiveTheme = computed(() => (theme.dark ? darkTheme : null))
const themeOverrides = computed(() => (theme.dark ? darkThemeOverrides : lightThemeOverrides))

onMounted(() => {
  auth.fetchMe()
})

const options = computed(() => [
  ...(auth.isAdmin ? [{ label: '系统管理', key: 'admin' }] : []),
  { label: '个人设置', key: 'settings' },
  { label: '我的项目', key: 'projects' },
  { label: '退出登录', key: 'logout' },
])

function onUserAction(key: string) {
  if (key === 'admin') router.push('/admin')
  else if (key === 'settings') router.push('/settings')
  else if (key === 'projects') router.push('/projects')
  else if (key === 'logout') {
    auth.logout()
    router.push('/')
  }
}

const isPub = ref(false)
watch(
  () => route.path,
  (p) => {
    isPub.value = p.startsWith('/pub')
  },
  { immediate: true },
)
</script>

<template>
  <n-config-provider
    :theme="naiveTheme"
    :theme-overrides="themeOverrides"
    :locale="zhCN"
    :date-locale="dateZhCN"
  >
    <n-global-style />
    <n-message-provider>
      <n-dialog-provider>
        <header class="app-topbar">
          <router-link to="/" class="brand">
            MDocs<span class="dot">.</span>
          </router-link>
          <span class="muted" style="font-size: 13px">Markdown 文档集管理</span>

          <nav class="nav-links">
            <router-link class="nav-link" to="/">首页</router-link>
            <router-link v-if="!isPub && auth.isLoggedIn" class="nav-link" to="/projects">我的项目</router-link>
            <router-link v-if="auth.isLoggedIn" class="nav-link" to="/settings">个人设置</router-link>
            <router-link class="nav-link" to="/about">关于</router-link>
          </nav>

          <span class="grow" />

          <n-button
            quaternary
            size="small"
            :title="theme.dark ? '切换到亮色' : '切换到暗色'"
            :aria-label="theme.dark ? '切换到亮色' : '切换到暗色'"
            @click="theme.toggle()"
          >
            <template #icon>
              <n-icon :component="theme.dark ? SunnyOutline : MoonOutline" />
            </template>
          </n-button>
          <template v-if="!auth.isLoggedIn">
            <n-button quaternary size="small" @click="router.push('/login')">登录</n-button>
            <n-button type="primary" size="small" @click="router.push('/register')">注册</n-button>
          </template>
          <n-dropdown v-else :options="options" @select="onUserAction">
            <n-avatar
              round
              :size="30"
              :src="auth.avatarUrl ?? undefined"
              style="cursor: pointer; font-weight: 600"
            >
              <!-- n-avatar 只要拿到默认插槽内容就只渲染文字、忽略 src，所以有头像时不能给默认插槽 -->
              <template v-if="!auth.avatarUrl">{{ auth.displayName?.[0]?.toUpperCase() || 'U' }}</template>
              <template #fallback>{{ auth.displayName?.[0]?.toUpperCase() || 'U' }}</template>
            </n-avatar>
          </n-dropdown>
        </header>

        <div style="min-height: calc(100vh - 56px)">
          <router-view />
        </div>
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>
