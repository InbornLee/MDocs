<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  NAlert,
  NAvatar,
  NButton,
  NForm,
  NFormItem,
  NInput,
  NMenu,
  NUpload,
  useDialog,
  useMessage,
} from 'naive-ui'
import type { MenuOption, UploadFileInfo } from 'naive-ui'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const message = useMessage()
const dialog = useDialog()

const displayName = ref(auth.user?.display_name ?? '')
const email = ref(auth.user?.email ?? '')
const bio = ref(auth.user?.bio ?? '')
const saving = ref(false)

async function save() {
  saving.value = true
  try {
    await api.patch('/api/me/profile', {
      display_name: displayName.value,
      email: email.value,
      bio: bio.value,
    })
    await auth.fetchMe()
    message.success('已保存')
  } catch (e: unknown) {
    message.error(String((e as Error)?.message ?? e))
  } finally {
    saving.value = false
  }
}

// n-upload 的 max 判定的是它内部 fileList 的长度：关闭自动上传时列表不会自己清空，
// 达到 max 后触发器会被禁用（点击连文件选择框都不打开），且没有任何提示。
// 所以这里用 v-model 接管列表，并在每次处理完后清空，保证可以继续换头像。
const avatarFileList = ref<UploadFileInfo[]>([])

async function onAvatarChange(options: { file?: UploadFileInfo | null }) {
  const file = options.file?.file
  if (!file) return
  const form = new FormData()
  form.append('avatar', file)
  try {
    await api.upload('/api/me/avatar', form)
    await auth.fetchMe()
    message.success('头像已更新')
  } catch (e: unknown) {
    message.error(String((e as Error)?.message ?? e))
  } finally {
    avatarFileList.value = []
  }
}

// ---- 修改密码 ----

const oldPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const changingPwd = ref(false)

async function changePassword() {
  if (!oldPassword.value) {
    message.warning('请输入当前密码')
    return
  }
  if (newPassword.value.length < 8) {
    message.warning('新密码至少 8 个字符')
    return
  }
  if (newPassword.value !== confirmPassword.value) {
    message.warning('两次输入的新密码不一致')
    return
  }
  if (newPassword.value === oldPassword.value) {
    message.warning('新密码不能与当前密码相同')
    return
  }
  changingPwd.value = true
  try {
    await api.put('/api/me/password', {
      old_password: oldPassword.value,
      new_password: newPassword.value,
    })
    oldPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    message.success('密码已修改')
  } catch (e: unknown) {
    message.error(String((e as Error)?.message ?? e))
  } finally {
    changingPwd.value = false
  }
}

// ---- API token（给 MCP 等外部客户端用）----

interface TokenItem {
  id: string
  name: string
  /** 用于辨认的前缀，完整 token 只在创建时返回一次 */
  prefix: string
  created_at: string
  last_used_at: string | null
}

const tokens = ref<TokenItem[]>([])
const tokenName = ref('')
const creatingToken = ref(false)
/** 新建出来的明文 token，只在这里显示一次 */
const newToken = ref('')
const tokensLoading = ref(false)

const mcpUrl = computed(() => `${window.location.origin}/mcp`)

/** 配置示例里用的 token：刚生成的直接填进去，否则留占位 */
const tokenForExample = computed(() => newToken.value || 'mdocs_在这里粘贴你的token')

const mcpJson = computed(() =>
  JSON.stringify(
    {
      mcpServers: {
        mdocs: {
          url: mcpUrl.value,
          headers: { Authorization: `Bearer ${tokenForExample.value}` },
        },
      },
    },
    null,
    2,
  ),
)

/** 常见客户端的配置文件位置（写配置前先按各自文档确认一下） */
const mcpClients = [
  {
    name: 'Claude Desktop',
    path: 'macOS ~/Library/Application Support/Claude/claude_desktop_config.json；Windows %APPDATA%\\Claude\\claude_desktop_config.json；Linux ~/.config/Claude/claude_desktop_config.json',
    note: '键名 mcpServers。若该客户端只支持 stdio，用 `npx mcp-remote <url> --header "Authorization: Bearer <token>"` 代理',
  },
  {
    name: 'Cursor',
    path: '全局 ~/.cursor/mcp.json；项目 .cursor/mcp.json',
    note: '键名 mcpServers',
  },
  {
    name: 'Trae',
    path: 'macOS ~/Library/Application Support/Trae/User/mcp.json；Windows %APPDATA%\\Trae\\User\\mcp.json；Linux ~/.config/Trae/User/mcp.json',
    note: '也可放项目内 .trae/mcp.json（需在设置里打开「启用项目级 MCP」）',
  },
  {
    name: 'VS Code (Copilot)',
    path: '工作区 .vscode/mcp.json；用户级：命令面板执行 MCP: Open User Configuration',
    note: '顶层键是 servers（不是 mcpServers），并需要 "type": "http"',
  },
  {
    name: 'Claude Code',
    path: '项目 .mcp.json；用户级 ~/.claude.json',
    note: '键名 mcpServers',
  },
]

function fmt(ts: string) {
  return new Date(ts).toLocaleString('zh-CN', { dateStyle: 'medium', timeStyle: 'short' })
}

async function loadTokens() {
  tokensLoading.value = true
  try {
    tokens.value = await api.get<TokenItem[]>('/api/me/tokens')
  } catch (e: unknown) {
    message.error(String((e as Error)?.message ?? e))
  } finally {
    tokensLoading.value = false
  }
}

async function createToken() {
  creatingToken.value = true
  try {
    const res = await api.post<{ token: string }>('/api/me/tokens', { name: tokenName.value })
    newToken.value = res.token
    tokenName.value = ''
    await loadTokens()
    message.success('已生成 token')
  } catch (e: unknown) {
    message.error(String((e as Error)?.message ?? e))
  } finally {
    creatingToken.value = false
  }
}

function revokeToken(t: TokenItem) {
  dialog.warning({
    title: '删除 token',
    content: `删除后使用「${t.name}」的客户端将立刻失效，确定删除吗？`,
    positiveText: '删除',
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await api.del(`/api/me/tokens/${t.id}`)
        await loadTokens()
        message.success('已删除')
      } catch (e: unknown) {
        message.error(String((e as Error)?.message ?? e))
      }
    },
  })
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    message.success('已复制到剪贴板')
  } catch {
    message.warning('复制失败，请手动选中复制')
  }
}

// ---- 左侧导航 ----

type SettingsTab = 'profile' | 'password' | 'token' | 'mcp'
const activeTab = ref<SettingsTab>('profile')

const menuOptions: MenuOption[] = [
  { label: '个人资料', key: 'profile' },
  { label: '修改密码', key: 'password' },
  { label: 'API token', key: 'token' },
  { label: 'MCP 配置', key: 'mcp' },
]

onMounted(loadTokens)
</script>

<template>
  <div class="page settings-page">
    <div class="page-head">
      <h2 class="page-title">个人设置</h2>
    </div>

    <div class="settings-layout">
      <aside class="card-surface settings-nav">
        <n-menu v-model:value="activeTab" :options="menuOptions" :indent="16" />
      </aside>

      <div class="settings-main">
        <section v-if="activeTab === 'profile'" class="card-surface settings-section">
          <div class="section-head">
            <span class="section-title">个人资料</span>
          </div>
          <div class="section-body">
            <div class="row" style="gap: 24px; margin-bottom: 16px">
              <n-avatar round :size="80" :src="auth.avatarUrl ?? undefined" style="font-size: 32px; font-weight: 600">
                <!-- n-avatar 只要拿到默认插槽内容就只渲染文字、忽略 src，所以有头像时不能给默认插槽 -->
                <template v-if="!auth.avatarUrl">{{ auth.displayName?.[0]?.toUpperCase() || 'U' }}</template>
                <template #fallback>{{ auth.displayName?.[0]?.toUpperCase() || 'U' }}</template>
              </n-avatar>
              <div>
                <n-upload
                  v-model:file-list="avatarFileList"
                  :max="1"
                  :default-upload="false"
                  :show-file-list="false"
                  accept="image/png,image/jpeg,image/webp"
                  @change="onAvatarChange"
                >
                  <n-button>上传新头像（png/jpg/webp，将裁剪为 512×512）</n-button>
                </n-upload>
                <p class="muted" style="font-size: 12px; margin: 8px 0 0">支持 PNG / JPEG / WebP，最大 5MB</p>
              </div>
            </div>
            <n-form>
              <n-form-item label="用户名">
                <n-input :value="auth.user?.username" disabled />
              </n-form-item>
              <n-form-item label="显示名称">
                <n-input v-model:value="displayName" placeholder="展示给他人的名称，留空则用用户名" />
              </n-form-item>
              <n-form-item label="邮箱">
                <n-input v-model:value="email" />
              </n-form-item>
              <n-form-item label="个人简介">
                <n-input v-model:value="bio" type="textarea" :rows="3" placeholder="一句话介绍自己" />
              </n-form-item>
              <n-button type="primary" :loading="saving" @click="save">保存</n-button>
            </n-form>
          </div>
        </section>

        <section v-else-if="activeTab === 'password'" class="card-surface settings-section">
          <div class="section-head">
            <span class="section-title">修改密码</span>
          </div>
          <div class="section-body">
            <n-form>
              <n-form-item label="当前密码">
                <n-input
                  v-model:value="oldPassword"
                  type="password"
                  show-password-on="click"
                  placeholder="当前使用的密码"
                />
              </n-form-item>
              <n-form-item label="新密码">
                <n-input
                  v-model:value="newPassword"
                  type="password"
                  show-password-on="click"
                  placeholder="至少 8 个字符"
                />
              </n-form-item>
              <n-form-item label="确认新密码">
                <n-input
                  v-model:value="confirmPassword"
                  type="password"
                  show-password-on="click"
                  placeholder="再输入一次新密码"
                />
              </n-form-item>
              <n-button type="primary" :loading="changingPwd" @click="changePassword">保存新密码</n-button>
            </n-form>
          </div>
        </section>

        <section v-else-if="activeTab === 'token'" class="card-surface settings-section">
          <div class="section-head">
            <span class="section-title">API token</span>
          </div>
          <div class="section-body">
            <p class="muted" style="font-size: 12.5px; margin: 0 0 14px; line-height: 1.7">
              供 MCP 等外部客户端调用。请求时带上
              <code class="mono">Authorization: Bearer &lt;token&gt;</code>，即可访问你参与的项目（含私有项目与未发布文档）；
              不带 token 只能看到公开项目与已发布文档。MCP 地址：
              <code class="mono">{{ mcpUrl }}</code>
            </p>

            <div class="row" style="gap: 8px; margin-bottom: 14px">
              <n-input
                v-model:value="tokenName"
                size="small"
                placeholder="名称，如 Claude Desktop"
                style="max-width: 260px"
                @keyup.enter="createToken"
              />
              <n-button size="small" type="primary" :loading="creatingToken" @click="createToken">
                生成 token
              </n-button>
            </div>

            <n-alert v-if="newToken" type="success" :show-icon="false" style="margin-bottom: 14px">
              <div class="row-between">
                <span>新 token（只显示这一次，请立即保存）</span>
                <n-button size="tiny" @click="copyText(newToken)">复制</n-button>
              </div>
              <code class="mono" style="display: block; margin-top: 8px; word-break: break-all">
                {{ newToken }}
              </code>
            </n-alert>

            <n-spin :show="tokensLoading">
              <p v-if="!tokens.length" class="empty-state" style="padding: 12px">还没有 token</p>
              <div
                v-for="t in tokens"
                :key="t.id"
                class="row-between wrap"
                style="padding: 8px 0; border-top: 1px solid var(--rule-soft)"
              >
                <div>
                  <div>{{ t.name }}</div>
                  <div class="muted" style="font-size: 12px">
                    <code class="mono">{{ t.prefix }}…</code> · 创建于 {{ fmt(t.created_at) }}
                    <template v-if="t.last_used_at"> · 最近使用 {{ fmt(t.last_used_at) }}</template>
                  </div>
                </div>
                <n-button size="tiny" type="error" ghost @click="revokeToken(t)">删除</n-button>
              </div>
            </n-spin>
          </div>
        </section>

        <section v-else class="card-surface settings-section">
          <div class="section-head">
            <span class="section-title">MCP 配置示例</span>
          </div>
          <div class="section-body">
            <p class="muted" style="font-size: 12.5px; margin: 0 0 10px; line-height: 1.7">
              下面这份是 Streamable HTTP 的写法，适用于键名为 <code class="mono">mcpServers</code> 的客户端
              （Claude Desktop / Cursor / Trae / Claude Code 等）；VS Code 的键名是
              <code class="mono">servers</code>，见表格备注。
              <template v-if="!newToken">
                token 请替换成上面「API token」里新生成的那串（只在创建时显示一次，之后只保留前缀）。
              </template>
            </p>

            <div class="row-between" style="margin-bottom: 6px">
              <span class="muted" style="font-size: 12px">mcp.json</span>
              <n-button size="tiny" @click="copyText(mcpJson)">复制</n-button>
            </div>
            <pre
              class="mono"
              style="
                margin: 0 0 16px;
                padding: 10px 12px;
                border: 1px solid var(--rule);
                border-radius: 6px;
                background: var(--bg-soft);
                font-size: 12px;
                line-height: 1.6;
                overflow-x: auto;
              "
              >{{ mcpJson }}</pre
            >

            <table style="width: 100%; border-collapse: collapse; font-size: 12.5px">
              <thead>
                <tr class="muted" style="text-align: left">
                  <th style="padding: 4px 10px 6px 0; font-weight: 500">客户端</th>
                  <th style="padding: 4px 10px 6px 0; font-weight: 500">配置文件</th>
                  <th style="padding: 4px 0 6px; font-weight: 500">备注</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="c in mcpClients"
                  :key="c.name"
                  style="border-top: 1px solid var(--rule-soft); vertical-align: top"
                >
                  <td style="padding: 6px 10px 6px 0; white-space: nowrap">{{ c.name }}</td>
                  <td style="padding: 6px 10px 6px 0">
                    <code class="mono">{{ c.path }}</code>
                  </td>
                  <td class="muted" style="padding: 6px 0">{{ c.note }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 左栏菜单 + 右侧内容，与系统管理页保持同一套布局 */
.settings-page {
  max-width: 1080px;
}
.page-head {
  display: flex;
  align-items: baseline;
  gap: 12px;
  margin-bottom: 16px;
}
.page-title {
  margin: 0;
}
.settings-layout {
  display: grid;
  grid-template-columns: 200px minmax(0, 1fr);
  gap: 16px;
  align-items: start;
}
.settings-nav {
  position: sticky;
  top: 72px;
  padding: 8px;
}
.settings-main {
  min-width: 0;
}
.settings-section {
  overflow: hidden;
}
.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--rule-soft);
}
.section-head .section-title {
  margin: 0;
}
.section-body {
  padding: 12px 16px;
}

@media (max-width: 760px) {
  .settings-layout {
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }
  .settings-nav {
    position: static;
  }
}
</style>
