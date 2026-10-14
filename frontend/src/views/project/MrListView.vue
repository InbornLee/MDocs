<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from 'vue'
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
import { useProjectStore } from '@/stores/project'
import { createMr, listMrs } from '@/api/projects'
import type { MrListItem } from '@/api/types'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const store = useProjectStore()

const slug = computed(() => route.params.slug as string)

const loading = ref(false)
const items = ref<MrListItem[]>([])
const statusFilter = ref<'open' | 'merged' | 'closed' | 'all'>('open')

const statusMeta: Record<string, { label: string; type: 'success' | 'warning' | 'default' | 'info' }> = {
  open: { label: '进行中', type: 'info' },
  merged: { label: '已合并', type: 'success' },
  closed: { label: '已关闭', type: 'default' },
}

function fmt(ts: string) {
  return new Date(ts).toLocaleString()
}

const columns = computed<DataTableColumns<MrListItem>>(() => [
  {
    title: '标题',
    key: 'title',
    render: (row) => row.title,
  },
  {
    title: '源 → 目标',
    key: 'branches',
    render: (row) =>
      `${row.source} → ${row.target}${row.target_is_default ? '（主）' : ''}`,
  },
  {
    title: '状态',
    key: 'status',
    width: 100,
    render: (row) => {
      const m = statusMeta[row.status] ?? { label: row.status, type: 'default' as const }
      return h(NTag, { size: 'small', type: m.type }, { default: () => m.label })
    },
  },
  {
    title: '冲突',
    key: 'conflict_files',
    width: 90,
    render: (row) =>
      row.status === 'open' && row.conflict_files.length
        ? h(NTag, { size: 'small', type: 'error' }, { default: () => `${row.conflict_files.length} 个` })
        : h('span', { class: 'muted' }, '-'),
  },
  { title: '提交人', key: 'created_by', width: 120 },
  { title: '创建时间', key: 'created_at', width: 180, render: (row) => fmt(row.created_at) },
  {
    title: '',
    key: 'actions',
    width: 90,
    render: (row) =>
      h(
        NButton,
        {
          size: 'tiny',
          onClick: () => router.push(`/p/${slug.value}/mrs/${row.id}`),
        },
        { default: () => '查看' },
      ),
  },
])

async function load() {
  loading.value = true
  try {
    items.value = await listMrs(slug.value, statusFilter.value)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch([slug, statusFilter], load)

// ---- create dialog ----
const showCreate = ref(false)
const creating = ref(false)
const form = ref({ source: '', target: '', title: '', description: '' })

const branchOptions = computed(() =>
  store.branches.map((b) => ({
    label: b.is_default ? `${b.name}（默认）` : b.name,
    value: b.name,
  })),
)

function openCreate() {
  const def = store.branches.find((b) => b.is_default)?.name ?? ''
  const other = store.branches.find((b) => b.name !== def)?.name ?? def
  const qSource = typeof route.query.source === 'string' ? route.query.source : ''
  form.value = {
    source: qSource || store.branchName || other,
    target: def,
    title: '',
    description: '',
  }
  showCreate.value = true
}

// auto-open the dialog when arriving with ?source=<branch>
onMounted(() => {
  if (typeof route.query.source === 'string' && route.query.source) openCreate()
})

async function submitCreate() {
  if (!form.value.title.trim()) {
    message.warning('请填写标题')
    return
  }
  if (form.value.source === form.value.target) {
    message.warning('源分支与目标分支不能相同')
    return
  }
  creating.value = true
  try {
    const res = await createMr(slug.value, {
      source: form.value.source,
      target: form.value.target || undefined,
      title: form.value.title.trim(),
      description: form.value.description.trim() || undefined,
    })
    const extra = res.conflicts.length ? `，${res.conflicts.length} 个文件存在冲突` : ''
    message.success(`已创建合并请求（${res.changed_files} 个文件变更${extra}）`)
    showCreate.value = false
    statusFilter.value = 'open'
    await load()
    router.push(`/p/${slug.value}/mrs/${res.id}`)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <div class="page">
    <section class="card-surface">
      <div class="row-between" style="padding: 12px 16px; border-bottom: 1px solid var(--rule-soft)">
        <span class="section-title" style="margin: 0">合并请求</span>
        <n-space align="center" :size="10">
          <n-select
            v-model:value="statusFilter"
            size="small"
            style="width: 130px"
            :options="[
              { label: '进行中', value: 'open' },
              { label: '已合并', value: 'merged' },
              { label: '已关闭', value: 'closed' },
              { label: '全部', value: 'all' },
            ]"
          />
          <n-button size="small" type="primary" @click="openCreate">新建合并请求</n-button>
        </n-space>
      </div>

      <div style="padding: 12px 16px">
        <n-data-table
          v-if="items.length"
          :columns="columns"
          :data="items"
          :loading="loading"
          :bordered="false"
          size="small"
          :row-key="(r: MrListItem) => r.id"
        />
        <div v-else class="empty-state">暂无合并请求</div>
      </div>
    </section>

    <n-modal
      v-model:show="showCreate"
      preset="dialog"
      title="新建合并请求"
      positive-text="创建"
      negative-text="取消"
      :loading="creating"
      style="width: 520px"
      @positive-click="submitCreate"
    >
      <n-space vertical :size="12" style="margin-top: 8px">
        <n-select
          v-model:value="form.source"
          :options="branchOptions"
          placeholder="源分支（你的分支）"
        />
        <n-select
          v-model:value="form.target"
          :options="branchOptions"
          placeholder="目标分支（默认主分支）"
        />
        <n-input v-model:value="form.title" placeholder="标题" />
        <n-input
          v-model:value="form.description"
          type="textarea"
          placeholder="描述（可选）"
          :rows="3"
        />
      </n-space>
    </n-modal>
  </div>
</template>
