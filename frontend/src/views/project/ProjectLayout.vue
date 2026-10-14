<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  NButton,
  NInput,
  NModal,
  NSelect,
  NSpace,
  NTag,
  useMessage,
} from 'naive-ui'
import { useProjectStore } from '@/stores/project'
import { createBranch, updateProject } from '@/api/projects'
import { roleLabel } from '@/utils/role'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const store = useProjectStore()

const slug = computed(() => route.params.slug as string)
const loading = ref(true)

const branchOptions = computed(() =>
  store.branches.map((b) => ({
    label: b.is_default ? `${b.name}（默认）` : b.name,
    value: b.name,
  })),
)

async function init() {
  loading.value = true
  store.$reset()
  try {
    await store.loadProject(slug.value)
    await store.loadDocs()
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}
onMounted(init)
watch(slug, init)

watch(
  () => store.branchName,
  async (nb, ob) => {
    if (!ob || nb === ob) return
    await store.loadDocs(nb)
  },
)

const navItems = computed(() => {
  const base = `/p/${slug.value}`
  return [
    { label: '文档', to: `${base}/tree` },
    { label: '链接图', to: `${base}/links` },
    { label: `分支 (${store.branches.length})`, to: `${base}/branches` },
    { label: '合并请求', to: `${base}/mrs` },
    { label: '成员', to: `${base}/members` },
    { label: '导入导出', to: `${base}/import-export` },
  ]
})

// ---- publish (project visibility) ----
const publishing = ref(false)

async function toggleVisibility() {
  if (!store.project) return
  publishing.value = true
  try {
    const next = store.project.visibility === 'public' ? 'private' : 'public'
    await updateProject(slug.value, { visibility: next })
    await store.loadProject(slug.value, { keepBranch: true })
    message.success(next === 'public' ? '项目已发布到公开站点' : '项目已取消发布')
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    publishing.value = false
  }
}

// ---- branch creation ----
const showNewBranch = ref(false)
const newBranch = ref({ name: '', from: '' })
const creatingBranch = ref(false)

function openNewBranch() {
  newBranch.value = { name: '', from: store.branchName }
  showNewBranch.value = true
}

async function submitNewBranch() {
  const name = newBranch.value.name.trim()
  if (!name) {
    message.warning('请输入分支名')
    return
  }
  creatingBranch.value = true
  try {
    const b = await createBranch(slug.value, {
      name,
      from_branch: newBranch.value.from || undefined,
    })
    message.success(`分支 ${b.name} 已创建`)
    showNewBranch.value = false
    await store.loadProject(slug.value, { keepBranch: true })
    store.branchName = b.name
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    creatingBranch.value = false
  }
}
</script>

<template>
  <div style="height: calc(100vh - 56px); display: flex; flex-direction: column">
    <!-- project header -->
    <div class="project-header">
      <div class="row wrap" style="gap: 12px">
        <h2 style="margin: 0; font-size: 19px">{{ store.project?.name ?? slug }}</h2>
        <n-tag
          v-if="store.project"
          size="small"
          :type="store.project.visibility === 'public' ? 'success' : 'default'"
        >
          {{ store.project.visibility === 'public' ? '公开' : '私有' }}
        </n-tag>
        <n-tag v-if="store.project && store.project.role !== 'owner'" size="small" type="info">
          {{ roleLabel(store.project.role) }}
        </n-tag>
        <span v-if="store.project?.description" class="muted" style="font-size: 13px">
          {{ store.project.description }}
        </span>
      </div>

      <div class="row wrap" style="margin-top: 10px; gap: 14px">
        <n-select
          v-model:value="store.branchName"
          :options="branchOptions"
          size="small"
          style="width: 200px"
        />
        <n-button size="small" @click="openNewBranch">新建分支</n-button>
        <n-button
          v-if="store.project && store.project.role === 'owner'"
          size="small"
          :loading="publishing"
          @click="toggleVisibility"
        >
          {{ store.project.visibility === 'public' ? '取消发布' : '发布到首页' }}
        </n-button>
        <n-button
          v-if="store.project?.visibility === 'public'"
          size="small"
          @click="router.push(`/pub/${slug}/tree`)"
        >
          公开站点
        </n-button>
        <nav class="nav-links" style="margin: 0; padding: 0; border: 0">
          <router-link v-for="item in navItems" :key="item.to" class="nav-link" :to="item.to">
            {{ item.label }}
          </router-link>
        </nav>
      </div>
    </div>

    <div style="flex: 1; min-height: 0; overflow: auto">
      <!-- keep children unmounted until the project + document list are loaded:
           DocView/DocEdit resolve the URL path against store.docs, and mounting
           them earlier made a hard reload of a document URL render nothing -->
      <router-view v-if="!loading" />
    </div>

    <n-modal
      v-model:show="showNewBranch"
      preset="dialog"
      title="新建分支"
      positive-text="创建"
      negative-text="取消"
      :loading="creatingBranch"
      @positive-click="submitNewBranch"
    >
      <n-space vertical style="margin-top: 8px">
        <n-input v-model:value="newBranch.name" placeholder="分支名，如 feature/xxx" />
        <n-select
          v-model:value="newBranch.from"
          :options="branchOptions"
          placeholder="基于哪个分支创建"
        />
      </n-space>
    </n-modal>
  </div>
</template>

<style scoped>
.project-header {
  padding: 14px 24px;
  border-bottom: 1px solid var(--rule);
  background: var(--bg);
}
</style>
