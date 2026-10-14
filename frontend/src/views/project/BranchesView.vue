<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  NButton,
  NDataTable,
  NInput,
  NModal,
  NSelect,
  NSpace,
  NTag,
  useMessage,
} from 'naive-ui'
import type { DataTableColumns } from 'naive-ui'
import { h } from 'vue'
import { useProjectStore } from '@/stores/project'
import { createBranch } from '@/api/projects'
import type { BranchDto } from '@/api/types'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const store = useProjectStore()

const slug = computed(() => route.params.slug as string)

const branches = ref<BranchDto[]>([])
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    await store.loadProject(slug.value, { keepBranch: true })
    branches.value = [...store.branches]
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(slug, load)

const branchOptions = computed(() =>
  store.branches.map((b) => ({
    label: b.is_default ? `${b.name}（默认）` : b.name,
    value: b.name,
  })),
)

const columns = computed<DataTableColumns<BranchDto>>(() => [
  {
    title: '分支',
    key: 'name',
    render: (row) =>
      h('div', { class: 'row' }, [
        h('span', { class: 'mono' }, row.name),
        row.is_default
          ? h(NTag, { size: 'tiny', type: 'success' }, { default: () => '默认' })
          : null,
      ]),
  },
  {
    title: '最新提交',
    key: 'head_commit_id',
    render: (row) =>
      row.head_commit_id
        ? h(
            'span',
            { class: 'mono muted', style: 'font-size: 12.5px' },
            row.head_commit_id.slice(0, 8),
          )
        : h('span', { class: 'muted' }, '（空分支）'),
  },
  {
    title: '创建时间',
    key: 'created_at',
    width: 180,
    render: (row) => new Date(row.created_at).toLocaleString(),
  },
  {
    title: '',
    key: 'actions',
    width: 220,
    render: (row) =>
      h(NSpace, { size: 6 }, {
        default: () => [
          h(
            NButton,
            {
              size: 'tiny',
              onClick: () => {
                store.branchName = row.name
                router.push(`/p/${slug.value}/tree`)
              },
            },
            { default: () => '浏览' },
          ),
          !row.is_default
            ? h(
                NButton,
                {
                  size: 'tiny',
                  onClick: () => {
                    openMr(row.name)
                  },
                },
                { default: () => '发起合并请求' },
              )
            : null,
        ],
      }),
  },
])

// ---- new branch ----
const showNew = ref(false)
const creating = ref(false)
const form = ref({ name: '', from: '' })

function openNew() {
  form.value = { name: '', from: store.branches.find((b) => b.is_default)?.name ?? '' }
  showNew.value = true
}

async function submitNew() {
  const name = form.value.name.trim()
  if (!name) {
    message.warning('请输入分支名')
    return
  }
  creating.value = true
  try {
    await createBranch(slug.value, { name, from_branch: form.value.from || undefined })
    message.success(`分支 ${name} 已创建`)
    showNew.value = false
    await load()
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    creating.value = false
  }
}

// ---- quick MR from a branch ----
function openMr(source: string) {
  router.push({ path: `/p/${slug.value}/mrs`, query: { source } })
}
</script>

<template>
  <div class="page">
    <section class="card-surface">
      <div class="row-between" style="padding: 12px 16px; border-bottom: 1px solid var(--rule-soft)">
        <span class="section-title" style="margin: 0">分支（{{ branches.length }}）</span>
        <n-button size="small" type="primary" @click="openNew">新建分支</n-button>
      </div>

      <div style="padding: 12px 16px">
        <n-data-table
          v-if="branches.length"
          :columns="columns"
          :data="branches"
          :loading="loading"
          :bordered="false"
          size="small"
          :row-key="(r: BranchDto) => r.id"
        />
        <div v-else class="empty-state">暂无分支</div>
      </div>
    </section>

    <n-modal
      v-model:show="showNew"
      preset="dialog"
      title="新建分支"
      positive-text="创建"
      negative-text="取消"
      :loading="creating"
      @positive-click="submitNew"
    >
      <n-space vertical :size="12" style="margin-top: 8px">
        <n-input v-model:value="form.name" placeholder="分支名，如 feature/xxx" />
        <n-select
          v-model:value="form.from"
          :options="branchOptions"
          placeholder="基于哪个分支创建"
        />
      </n-space>
    </n-modal>
  </div>
</template>
