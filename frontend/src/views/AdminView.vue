<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  NButton,
  NDataTable,
  NInput,
  NMenu,
  NPagination,
  NResult,
  NSpace,
  NTag,
  useDialog,
  useMessage,
} from 'naive-ui'
import type { DataTableColumns, MenuOption } from 'naive-ui'
import { useAuthStore } from '@/stores/auth'
import {
  listAdminProjects,
  listAdminUsers,
  resetUserPassword,
  setUserEnabled,
  unpublishProject,
  updateSiteSettings,
} from '@/api/admin'
import { getSiteSettings } from '@/api/public'
import type { AdminProjectDto, AdminUserDto } from '@/api/types'

const auth = useAuthStore()
const router = useRouter()
const message = useMessage()
const dialog = useDialog()

const isAdmin = computed(() => auth.isAdmin)

// ---------- 左侧导航 ----------
type AdminTab = 'site' | 'users' | 'projects'
const activeTab = ref<AdminTab>('site')

// ---------- 站点设置 ----------
const homeTitle = ref('')
/** 关于页的仓库与反馈入口，留空表示不显示 */
const repoUrl = ref('')
const feedbackUrl = ref('')
const feedbackEmail = ref('')
const savingTitle = ref(false)

async function loadSite() {
  try {
    const s = await getSiteSettings()
    homeTitle.value = s.home_title
    repoUrl.value = s.repo_url ?? ''
    feedbackUrl.value = s.feedback_url ?? ''
    feedbackEmail.value = s.feedback_email ?? ''
  } catch (e: unknown) {
    message.error((e as Error).message)
  }
}

async function saveSite() {
  const title = homeTitle.value.trim()
  if (!title) {
    message.warning('首页标题不能为空')
    return
  }
  for (const [label, value] of [
    ['仓库地址', repoUrl.value.trim()],
    ['反馈链接', feedbackUrl.value.trim()],
  ] as const) {
    if (value && !/^https?:\/\//.test(value)) {
      message.warning(`${label}需要以 http:// 或 https:// 开头`)
      return
    }
  }
  const email = feedbackEmail.value.trim()
  if (email && !email.includes('@')) {
    message.warning('反馈邮箱格式不正确')
    return
  }

  savingTitle.value = true
  try {
    const s = await updateSiteSettings({
      home_title: title,
      repo_url: repoUrl.value.trim(),
      feedback_url: feedbackUrl.value.trim(),
      feedback_email: email,
    })
    homeTitle.value = s.home_title
    repoUrl.value = s.repo_url ?? ''
    feedbackUrl.value = s.feedback_url ?? ''
    feedbackEmail.value = s.feedback_email ?? ''
    message.success('站点设置已更新')
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    savingTitle.value = false
  }
}

// ---------- 注册成员 ----------
const users = ref<AdminUserDto[]>([])
const usersLoading = ref(false)
const userQuery = ref('')
const userPage = ref(1)
const userTotal = ref(0)
const userPageSize = 20

async function loadUsers() {
  usersLoading.value = true
  try {
    const data = await listAdminUsers({
      query: userQuery.value.trim() || undefined,
      page: userPage.value,
      pageSize: userPageSize,
    })
    users.value = data.items
    userTotal.value = data.total
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    usersLoading.value = false
  }
}

function searchUsers() {
  userPage.value = 1
  loadUsers()
}

function confirmToggleUser(row: AdminUserDto) {
  const action = row.enabled ? '禁用' : '启用'
  dialog.warning({
    title: `${action}账号`,
    content: row.enabled
      ? `禁用后 @${row.username} 将无法登录，已登录的会话也会立即失效。`
      : `启用后 @${row.username} 可以重新登录。`,
    positiveText: action,
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await setUserEnabled(row.id, !row.enabled)
        message.success(`已${action} @${row.username}`)
        await loadUsers()
      } catch (e: unknown) {
        message.error((e as Error).message)
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

/** 重置成功后单独弹一次，明文密码只能在这里看到 */
function showNewPassword(username: string, password: string) {
  dialog.success({
    title: '密码已重置',
    content: () =>
      h('div', {}, [
        h('div', { style: 'margin-bottom: 8px' }, `@${username} 的新密码（只显示这一次）：`),
        h(
          'code',
          {
            class: 'mono',
            style:
              'display:block; padding:8px 10px; border:1px solid var(--rule); border-radius:6px;' +
              ' background:var(--bg-soft); font-size:15px; letter-spacing:1px',
          },
          password,
        ),
        h(
          'div',
          { class: 'muted', style: 'margin-top:8px; font-size:12px' },
          '请复制后转告该成员，关闭后无法再次查看。',
        ),
      ]),
    positiveText: '复制密码',
    negativeText: '关闭',
    onPositiveClick: async () => {
      await copyText(password)
    },
  })
}

function confirmResetPassword(row: AdminUserDto) {
  dialog.warning({
    title: '重置密码',
    content: `将给 @${row.username} 生成一个 8 位随机密码，对方当前密码立即失效。确定重置吗？`,
    positiveText: '重置',
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        const res = await resetUserPassword(row.id)
        showNewPassword(res.username, res.password)
      } catch (e: unknown) {
        message.error((e as Error).message)
      }
    },
  })
}

const userColumns = computed<DataTableColumns<AdminUserDto>>(() => [
  {
    title: '成员',
    key: 'username',
    render: (row) =>
      h('div', { class: 'row' }, [
        h('span', {}, row.display_name || row.username),
        h('span', { class: 'muted', style: 'font-size: 12px' }, `@${row.username}`),
      ]),
  },
  {
    title: '邮箱',
    key: 'email',
    render: (row) => h('span', { class: 'muted' }, row.email),
  },
  { title: '项目', key: 'project_count', width: 80 },
  {
    title: '注册时间',
    key: 'created_at',
    width: 170,
    render: (row) => new Date(row.created_at).toLocaleString('zh-CN'),
  },
  {
    title: '状态',
    key: 'enabled',
    width: 100,
    render: (row) => {
      if (row.is_admin) {
        return h(NTag, { size: 'small', type: 'success' }, { default: () => '管理员' })
      }
      return h(
        NTag,
        { size: 'small', type: row.enabled ? 'default' : 'error' },
        { default: () => (row.enabled ? '正常' : '已禁用') },
      )
    },
  },
  {
    title: '',
    key: 'actions',
    width: 160,
    // 内置管理员不允许被禁用 / 重置，所以没有操作按钮
    render: (row) => {
      if (row.is_admin) return null
      return h(
        NSpace,
        { size: 4, align: 'center', wrap: false },
        {
          default: () => [
            h(
              NButton,
              {
                size: 'tiny',
                quaternary: true,
                onClick: () => confirmResetPassword(row),
              },
              { default: () => '重置密码' },
            ),
            h(
              NButton,
              {
                size: 'tiny',
                quaternary: true,
                type: row.enabled ? 'error' : 'primary',
                onClick: () => confirmToggleUser(row),
              },
              { default: () => (row.enabled ? '禁用' : '启用') },
            ),
          ],
        },
      )
    },
  },
])

// ---------- 公开项目 ----------
const projects = ref<AdminProjectDto[]>([])
const projectsLoading = ref(false)
const projectQuery = ref('')
const projectPage = ref(1)
const projectTotal = ref(0)
const projectPageSize = 20

async function loadProjects() {
  projectsLoading.value = true
  try {
    const data = await listAdminProjects({
      query: projectQuery.value.trim() || undefined,
      page: projectPage.value,
      pageSize: projectPageSize,
    })
    projects.value = data.items
    projectTotal.value = data.total
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    projectsLoading.value = false
  }
}

function searchProjects() {
  projectPage.value = 1
  loadProjects()
}

function confirmUnpublish(row: AdminProjectDto) {
  dialog.warning({
    title: '取消公开',
    content: `将「${row.name}」改回私有？首页与公开链接会立刻不可访问，文档本身不会被删除。`,
    positiveText: '取消公开',
    negativeText: '再想想',
    onPositiveClick: async () => {
      try {
        await unpublishProject(row.slug)
        message.success(`已取消公开 ${row.name}`)
        await loadProjects()
      } catch (e: unknown) {
        message.error((e as Error).message)
      }
    },
  })
}

const projectColumns = computed<DataTableColumns<AdminProjectDto>>(() => [
  {
    title: '项目',
    key: 'name',
    render: (row) =>
      h('div', { class: 'row' }, [
        h('span', {}, row.name),
        h('span', { class: 'muted', style: 'font-size: 12px' }, row.slug),
      ]),
  },
  { title: '拥有者', key: 'owner_name', width: 160 },
  {
    title: '描述',
    key: 'description',
    render: (row) => h('span', { class: 'muted' }, row.description || '—'),
  },
  {
    title: '更新时间',
    key: 'updated_at',
    width: 170,
    render: (row) => new Date(row.updated_at).toLocaleString('zh-CN'),
  },
  {
    title: '',
    key: 'actions',
    width: 110,
    render: (row) =>
      h(
        NButton,
        {
          size: 'tiny',
          quaternary: true,
          type: 'error',
          onClick: () => confirmUnpublish(row),
        },
        { default: () => '取消公开' },
      ),
  },
])

// ---------- 初始化 ----------
// 菜单里要显示条目数，所以三个区块的数据在进入页面时一起加载
const menuOptions = computed<MenuOption[]>(() => [
  { label: '站点设置', key: 'site' },
  { label: `注册成员（${userTotal.value}）`, key: 'users' },
  { label: `公开项目（${projectTotal.value}）`, key: 'projects' },
])

let initialized = false

function init() {
  if (initialized) return
  initialized = true
  loadSite()
  loadUsers()
  loadProjects()
}

onMounted(() => {
  // 直接打开 /admin 时 auth.user 可能还没回来，等它加载完再判断
  if (auth.loaded) {
    if (isAdmin.value) init()
  }
})

watch(
  () => auth.loaded && isAdmin.value,
  (ready) => {
    if (ready) init()
  },
)
</script>

<template>
  <div class="page admin-page">
    <div class="page-head">
      <h2 class="page-title">系统管理</h2>
      <span class="muted" style="font-size: 13px">管理员专属</span>
    </div>

    <n-result
      v-if="auth.loaded && !isAdmin"
      status="403"
      title="无权访问"
      description="只有内置的管理员账号可以进入管理界面"
    >
      <template #footer>
        <n-button @click="router.push('/')">返回首页</n-button>
      </template>
    </n-result>

    <template v-else>
      <div class="admin-layout">
        <aside class="card-surface admin-nav">
          <n-menu v-model:value="activeTab" :options="menuOptions" :indent="16" />
        </aside>

        <div class="admin-main">
          <section v-if="activeTab === 'site'" class="card-surface admin-section">
            <div class="section-head">
              <span class="section-title">站点设置</span>
            </div>
            <div class="section-body">
              <n-space vertical :size="10">
                <div class="row" style="gap: 12px">
                  <span class="field-label">首页标题</span>
                  <n-input v-model:value="homeTitle" style="width: 260px" placeholder="公开项目" />
                </div>
                <div class="row" style="gap: 12px">
                  <span class="field-label">仓库地址</span>
                  <n-input v-model:value="repoUrl" style="width: 420px" placeholder="https://…（留空则不显示）" />
                </div>
                <div class="row" style="gap: 12px">
                  <span class="field-label">反馈链接</span>
                  <n-input v-model:value="feedbackUrl" style="width: 420px" placeholder="https://…（如 Issue 页，留空则不显示）" />
                </div>
                <div class="row" style="gap: 12px">
                  <span class="field-label">反馈邮箱</span>
                  <n-input v-model:value="feedbackEmail" style="width: 420px" placeholder="you@example.com（留空则不显示）" />
                </div>
                <n-button type="primary" :loading="savingTitle" @click="saveSite">保存</n-button>
              </n-space>
              <p class="muted hint">
                首页标题显示在首页顶部（1-64 字符，默认「公开项目」）；仓库地址与反馈入口展示在
                <router-link to="/about">关于</router-link> 页，留空即不显示。
              </p>
            </div>
          </section>

          <section v-else-if="activeTab === 'users'" class="card-surface admin-section">
            <div class="section-head">
              <span class="section-title">注册成员（{{ userTotal }}）</span>
              <n-space :size="8" align="center">
                <n-input
                  v-model:value="userQuery"
                  size="small"
                  placeholder="搜索用户名 / 邮箱 / 昵称"
                  style="width: 220px"
                  @keyup.enter="searchUsers"
                />
                <n-button size="small" @click="searchUsers">搜索</n-button>
              </n-space>
            </div>
            <div class="section-body">
              <n-data-table
                :columns="userColumns"
                :data="users"
                :loading="usersLoading"
                :bordered="false"
                size="small"
                :row-key="(r: AdminUserDto) => r.id"
              />
              <n-space v-if="userTotal > userPageSize" justify="center" class="pager">
                <n-pagination
                  v-model:page="userPage"
                  :page-count="Math.ceil(userTotal / userPageSize)"
                  @update:page="loadUsers"
                />
              </n-space>
            </div>
          </section>

          <section v-else class="card-surface admin-section">
            <div class="section-head">
              <span class="section-title">公开项目（{{ projectTotal }}）</span>
              <n-space :size="8" align="center">
                <n-input
                  v-model:value="projectQuery"
                  size="small"
                  placeholder="搜索项目名称 / slug"
                  style="width: 220px"
                  @keyup.enter="searchProjects"
                />
                <n-button size="small" @click="searchProjects">搜索</n-button>
              </n-space>
            </div>
            <div class="section-body">
              <n-data-table
                :columns="projectColumns"
                :data="projects"
                :loading="projectsLoading"
                :bordered="false"
                size="small"
                :row-key="(r: AdminProjectDto) => r.id"
              />
              <n-space v-if="projectTotal > projectPageSize" justify="center" class="pager">
                <n-pagination
                  v-model:page="projectPage"
                  :page-count="Math.ceil(projectTotal / projectPageSize)"
                  @update:page="loadProjects"
                />
              </n-space>
            </div>
          </section>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.admin-page {
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
.admin-layout {
  display: grid;
  grid-template-columns: 200px minmax(0, 1fr);
  gap: 16px;
  align-items: start;
}
.admin-nav {
  position: sticky;
  top: 72px;
  padding: 8px;
}
.admin-main {
  min-width: 0;
}
.admin-section {
  margin-bottom: 16px;
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
.field-label {
  font-size: 13px;
}
.hint {
  margin: 8px 0 0;
  font-size: 12.5px;
}
.pager {
  margin-top: 12px;
}
@media (max-width: 760px) {
  .admin-layout {
    grid-template-columns: 1fr;
  }
  .admin-nav {
    position: static;
  }
}
</style>
