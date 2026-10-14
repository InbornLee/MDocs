<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  NAlert,
  NButton,
  NEmpty,
  NGi,
  NGrid,
  NInput,
  NModal,
  NPopconfirm,
  NSelect,
  NSpace,
  NTag,
  useMessage,
} from 'naive-ui'
import type { ProjectDto } from '@/api/types'
import { createProject, deleteProject, leaveProject, listProjects } from '@/api/projects'
import { roleLabel } from '@/utils/role'

const router = useRouter()
const message = useMessage()

const projects = ref<ProjectDto[]>([])
const loading = ref(false)
const showCreate = ref(false)
const creating = ref(false)
const form = ref({
  name: '',
  slug: '',
  description: '',
  visibility: 'private',
})

const visOptions = [
  { label: '私有 - 仅成员可见', value: 'private' },
  { label: '公开 - 发布至首页', value: 'public' },
]

// ---------- slug suggestion ----------
// Mirrors the server rule (`slugify` in routes/project.rs): keep [a-z0-9], fold
// every other character into a single dash, drop trailing dashes. A name
// without any ascii letter/digit — a pure Chinese name, for instance — yields
// nothing, and the server would reject the empty slug, so fill a suggestion the
// user can still edit.
function slugify(name: string) {
  let out = ''
  let prevDash = true // also trims leading dashes
  for (const ch of name.toLowerCase()) {
    if ((ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9')) {
      out += ch
      prevDash = false
    } else if (!prevDash) {
      out += '-'
      prevDash = true
    }
  }
  return out.replace(/-+$/, '')
}

// true while the slug field is ours to fill; typing in it hands control back
const slugAuto = ref(true)
const slugSeed = ref('')

const nameNeedsSlug = computed(
  () => form.value.name.trim().length > 0 && slugify(form.value.name) === '',
)
const suggestedSlug = computed(() => slugify(form.value.name) || `project-${slugSeed.value}`)

watch(
  () => form.value.name,
  () => {
    if (slugAuto.value) form.value.slug = form.value.name.trim() ? suggestedSlug.value : ''
  },
)

function onSlugChange(value: string) {
  form.value.slug = value
  slugAuto.value = false
}

function openCreate() {
  slugSeed.value = Math.random().toString(36).slice(2, 8)
  slugAuto.value = true
  form.value = { name: '', slug: '', description: '', visibility: 'private' }
  showCreate.value = true
}

async function refresh() {
  loading.value = true
  try {
    projects.value = await listProjects()
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}
onMounted(refresh)

async function submitCreate() {
  const name = form.value.name.trim()
  if (!name) {
    message.warning('请输入项目名称')
    return
  }
  // nothing to derive from and the user cleared the field: put the suggestion
  // back instead of letting the request fail with the server's slug error
  if (!form.value.slug.trim() && slugify(name) === '') {
    slugAuto.value = true
    form.value.slug = suggestedSlug.value
    message.warning(`项目名称无法生成 slug，已填入建议值 ${suggestedSlug.value}，可直接修改后再创建`)
    return
  }
  creating.value = true
  try {
    const p = await createProject({
      name,
      slug: form.value.slug.trim() || undefined,
      description: form.value.description.trim() || undefined,
      visibility: form.value.visibility,
    })
    message.success(`项目 ${p.slug} 创建成功`)
    showCreate.value = false
    form.value = { name: '', slug: '', description: '', visibility: 'private' }
    router.push(`/p/${p.slug}/tree`)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    creating.value = false
  }
}

async function removeProject(p: ProjectDto) {
  try {
    await deleteProject(p.slug)
    message.success('已删除')
    await refresh()
  } catch (e: unknown) {
    message.error((e as Error).message)
  }
}

/** 退出合作项目：把自己从成员里移除 */
async function exitProject(p: ProjectDto) {
  try {
    await leaveProject(p.slug)
    message.success(`已退出 ${p.name}`)
    await refresh()
  } catch (e: unknown) {
    message.error((e as Error).message)
  }
}

function fmtDate(s: string) {
  return new Date(s).toLocaleString('zh-CN', { dateStyle: 'short', timeStyle: 'short' })
}
</script>

<template>
  <div class="page">
    <div class="row-between page-head">
      <h2 class="page-title">我的项目</h2>
      <n-button type="primary" @click="openCreate">新建项目</n-button>
    </div>

    <n-empty
      v-if="!loading && projects.length === 0"
      class="empty-state"
      description="还没有项目，点击右上角创建一个吧"
    />

    <n-grid v-else :x-gap="16" :y-gap="16" cols="1 s:2 m:3" responsive="screen">
      <n-gi v-for="p in projects" :key="p.id">
        <div class="card-surface project-card" @click="router.push(`/p/${p.slug}/tree`)">
          <div class="row-between">
            <span class="grow ellipsis project-name">{{ p.name }}</span>
            <n-space size="small" align="center">
              <n-tag v-if="!p.is_creator" size="small" type="warning">合作项目</n-tag>
              <n-tag size="small" :type="p.visibility === 'public' ? 'success' : 'default'">
                {{ p.visibility === 'public' ? '公开' : '私有' }}
              </n-tag>
            </n-space>
          </div>
          <p class="muted project-desc">
            {{ p.description || '暂无描述' }}
          </p>
          <div class="row-between project-foot">
            <n-space size="small" align="center">
              <n-tag size="small" :bordered="false">{{ p.slug }}</n-tag>
              <n-tag size="small" :bordered="false" type="info">{{ roleLabel(p.role) }}</n-tag>
            </n-space>
            <n-space size="small" align="center">
              <n-popconfirm v-if="p.is_owner" @positive-click="removeProject(p)" @click.stop>
                <template #trigger>
                  <n-button size="tiny" quaternary type="error" @click.stop>删除</n-button>
                </template>
                删除项目 {{ p.name }} 及其所有文档、版本历史，不可恢复？
              </n-popconfirm>
              <n-popconfirm v-if="!p.is_creator" @positive-click="exitProject(p)" @click.stop>
                <template #trigger>
                  <n-button size="tiny" quaternary @click.stop>退出项目</n-button>
                </template>
                退出项目 {{ p.name }}？退出后将无法访问该项目。
              </n-popconfirm>
            </n-space>
          </div>
          <div class="muted project-time">更新于 {{ fmtDate(p.updated_at) }}</div>
        </div>
      </n-gi>
    </n-grid>

    <n-modal
      v-model:show="showCreate"
      preset="dialog"
      title="新建项目"
      positive-text="创建"
      negative-text="取消"
      :loading="creating"
      @positive-click="submitCreate"
    >
      <n-space vertical class="create-form">
        <n-input v-model:value="form.name" placeholder="项目名称（必填）" />
        <n-input
          :value="form.slug"
          placeholder="slug（可选，默认由名称生成，a-z0-9-）"
          @update:value="onSlugChange"
        />
        <n-alert v-if="nameNeedsSlug" type="warning" :show-icon="false" class="slug-alert">
          <template v-if="slugAuto">
            项目名称里没有 a-z / 0-9，无法自动生成 slug，已填入建议值
            <code>{{ form.slug }}</code>，可直接修改（仅 a-z0-9-）。
          </template>
          <template v-else>
            项目名称里没有 a-z / 0-9，slug 需要手动填写（仅 a-z0-9-）。
          </template>
        </n-alert>
        <n-input v-model:value="form.description" type="textarea" :rows="2" placeholder="项目描述（可选）" />
        <n-select v-model:value="form.visibility" :options="visOptions" />
      </n-space>
    </n-modal>
  </div>
</template>

<style scoped>
.page-head {
  margin-bottom: 16px;
}
.page-title {
  margin: 0;
}
.project-card {
  display: flex;
  flex-direction: column;
  padding: 14px 16px;
  cursor: pointer;
  transition: border-color 0.16s, box-shadow 0.16s;
}
.project-card:hover {
  border-color: var(--brand);
  box-shadow: var(--shadow-sm);
}
.project-name {
  font-weight: 600;
}
.project-desc {
  margin: 8px 0 0;
  font-size: 13px;
  min-height: 20px;
}
.project-foot {
  margin-top: 12px;
}
.project-time {
  margin-top: 8px;
  font-size: 12px;
  font-family: var(--mono);
}
.create-form {
  margin-top: 8px;
}
.slug-alert {
  font-size: 12.5px;
}
</style>
