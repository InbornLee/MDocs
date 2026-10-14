<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  NAlert,
  NButton,
  NModal,
  NSelect,
  NSpin,
  NTag,
  useDialog,
  useMessage,
} from 'naive-ui'
import { useProjectStore } from '@/stores/project'
import { getDocDiff, getDocHistory, revertDoc } from '@/api/projects'
import { createMarkdownIt, encodeDocPath, renderMermaid } from '@/utils/markdown'
import DocOutline from '@/components/DocOutline.vue'
import type { DiffDto, RevisionDto } from '@/api/types'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const dialog = useDialog()
const store = useProjectStore()

const slug = computed(() => route.params.slug as string)
const docPath = computed(() => {
  const m = route.params.pathMatch
  return Array.isArray(m) ? m.join('/') : (m ?? '')
})

const docId = computed(() => store.docByPath(docPath.value)?.id ?? '')

const loading = ref(false)
const revisions = ref<RevisionDto[]>([])

const changeMeta: Record<string, { label: string; type: 'success' | 'warning' | 'error' | 'info' }> = {
  added: { label: '新增', type: 'success' },
  modified: { label: '修改', type: 'warning' },
  deleted: { label: '删除', type: 'error' },
  renamed: { label: '重命名', type: 'info' },
}

const commitTypeLabel: Record<string, string> = {
  edit: '编辑',
  merge: '合并',
  revert: '回溯',
  import: '导入',
}

function fmt(ts: string) {
  return new Date(ts).toLocaleString('zh-CN', { dateStyle: 'medium', timeStyle: 'short' })
}

function short(id: string | null | undefined) {
  return id ? id.slice(0, 8) : '—'
}

async function load() {
  selected.value = null
  previewContent.value = ''
  if (!docId.value) {
    revisions.value = []
    loading.value = false
    return
  }
  loading.value = true
  try {
    revisions.value = await getDocHistory(slug.value, docId.value, store.branchName)
    // 默认预览当前版本，右侧不留空白
    await selectRevision(revisions.value.find((r) => r.current) ?? revisions.value[0] ?? null)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

// the doc list is needed to map the URL path -> doc id
async function init() {
  if (!store.docs.length) {
    try {
      await store.loadProject(slug.value, { keepBranch: true })
      await store.loadDocs()
    } catch (e: unknown) {
      message.error((e as Error).message)
    }
  }
  await load()
}

onMounted(init)
watch([docPath, () => store.branchName], load)
watch(docId, load)

// ---------- 历史版本预览 ----------
const md = createMarkdownIt(() => ({
  projectId: store.project?.id,
  docPath: docPath.value,
}))
/** 左侧列表里选中的版本，右侧预览它 */
const selected = ref<RevisionDto | null>(null)
const previewContent = ref('')
const previewLoading = ref(false)
/** 渲染后的正文容器，用来生成右侧大纲 */
const previewBodyEl = ref<HTMLElement | null>(null)

const previewHtml = computed(() => (previewContent.value ? md.render(previewContent.value) : ''))

// ```mermaid blocks are drawn once the preview is in the DOM
watch(previewHtml, async () => {
  await nextTick()
  await renderMermaid(previewBodyEl.value)
})

/** 选中某个历史版本并加载它的只读预览内容 */
async function selectRevision(rev: RevisionDto | null) {
  selected.value = rev
  previewContent.value = ''
  if (!rev || rev.change_type === 'deleted') return
  previewLoading.value = true
  try {
    // 没有单独取某版本内容的接口，diff 接口按 `to` 返回该版本的完整内容
    const d = await getDocDiff(slug.value, docId.value, {
      branch: store.branchName,
      from: null,
      to: rev.commit_id,
    })
    previewContent.value = d.new_content
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    previewLoading.value = false
  }
}

// ---------- diff ----------
const showDiff = ref(false)
const diffLoading = ref(false)
const diff = ref<DiffDto | null>(null)
const diffTarget = ref<RevisionDto | null>(null)
const diffBaseId = ref<string | null>(null)

const baseOptions = computed(() =>
  revisions.value
    .filter((r) => !diffTarget.value || r.commit_id !== diffTarget.value.commit_id)
    .map((r) => ({
      label: `${short(r.commit_id)} · ${changeMeta[r.change_type]?.label ?? r.change_type} · ${r.message.slice(0, 40)}`,
      value: r.commit_id,
    })),
)

async function openDiff(target: RevisionDto) {
  diffTarget.value = target
  // default base: the revision right after the target (older) or current
  const i = revisions.value.findIndex((r) => r.commit_id === target.commit_id)
  const older = revisions.value[i + 1]
  diffBaseId.value = older ? older.commit_id : null
  showDiff.value = true
  await runDiff()
}

async function runDiff() {
  if (!docId.value || !diffTarget.value) return
  diffLoading.value = true
  try {
    diff.value = await getDocDiff(slug.value, docId.value, {
      branch: store.branchName,
      from: diffBaseId.value,
      to: diffTarget.value.commit_id,
      context: 3,
    })
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    diffLoading.value = false
  }
}

watch(diffBaseId, () => {
  if (showDiff.value) runDiff()
})

interface DiffRow {
  kind: 'add' | 'del' | 'ctx' | 'hunk'
  text: string
  oldNo?: number
  newNo?: number
}

const diffRows = computed<DiffRow[]>(() => {
  if (!diff.value?.diff) return []
  const rows: DiffRow[] = []
  let oldNo = 0
  let newNo = 0
  for (const line of diff.value.diff.split('\n')) {
    if (line === '') continue
    if (line.startsWith('@@')) {
      const m = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line)
      if (m) {
        oldNo = Number(m[1])
        newNo = Number(m[2])
      }
      rows.push({ kind: 'hunk', text: line })
      continue
    }
    const head = line[0]
    const body = line.slice(1)
    if (head === '+') {
      rows.push({ kind: 'add', text: body, newNo: newNo++ })
    } else if (head === '-') {
      rows.push({ kind: 'del', text: body, oldNo: oldNo++ })
    } else {
      rows.push({ kind: 'ctx', text: body, oldNo: oldNo++, newNo: newNo++ })
    }
  }
  return rows
})

// ---------- revert ----------
const reverting = ref(false)

function confirmRevert(rev: RevisionDto) {
  if (rev.change_type === 'deleted') {
    message.warning('该修订是删除操作，无法作为回溯目标')
    return
  }
  dialog.warning({
    title: '回溯到该版本',
    content: `将把 ${docPath.value} 的内容恢复为修订 ${short(rev.commit_id)}（${fmt(rev.created_at)}），并生成一条新的提交记录。`,
    positiveText: '回溯',
    negativeText: '取消',
    onPositiveClick: async () => {
      reverting.value = true
      try {
        const r = await revertDoc(slug.value, docId.value, {
          commit_id: rev.commit_id,
          branch: store.branchName || undefined,
        })
        if (r.unchanged) {
          message.info('当前内容与该版本一致，无需回溯')
        } else {
          message.success(`已回溯到 ${short(rev.commit_id)}`)
        }
        await store.loadDocs()
        await load()
      } catch (e: unknown) {
        message.error((e as Error).message)
      } finally {
        reverting.value = false
      }
    },
  })
}
</script>

<template>
  <div class="reader-shell" style="height: 100%">
    <!-- 左：版本列表 -->
    <aside class="reader-aside">
      <div class="row-between" style="margin-bottom: 8px">
        <span class="section-title" style="margin: 0">版本历史</span>
        <n-button
          size="tiny"
          quaternary
          @click="router.push(`/p/${slug}/view/${encodeDocPath(docPath)}`)"
        >
          返回阅读
        </n-button>
      </div>
      <div class="row wrap" style="gap: 6px; margin-bottom: 10px">
        <code class="mono" style="font-size: 12px">{{ docPath }}</code>
        <n-tag size="tiny" :bordered="false">分支 {{ store.branchName || '默认' }}</n-tag>
      </div>

      <n-alert v-if="!docId" type="warning" :show-icon="false">
        文档不在此分支中，请切换分支后重试。
      </n-alert>

      <n-spin v-else :show="loading">
        <p v-if="!revisions.length && !loading" class="empty-state">暂无版本记录</p>

        <div
          v-for="r in revisions"
          :key="r.commit_id"
          class="version-item"
          :class="{ active: selected?.commit_id === r.commit_id }"
          role="button"
          tabindex="0"
          @click="selectRevision(r)"
          @keydown.enter="selectRevision(r)"
        >
          <div class="version-line">
            <span class="version-msg">{{ r.message }}</span>
            <span class="version-time">{{ fmt(r.created_at) }}</span>
          </div>

          <div class="version-meta">
            <n-tag size="tiny" :type="changeMeta[r.change_type]?.type ?? 'default'">
              {{ changeMeta[r.change_type]?.label ?? r.change_type }}
            </n-tag>
            <n-tag v-if="r.commit_type && r.commit_type !== 'edit'" size="tiny" :bordered="false">
              {{ commitTypeLabel[r.commit_type] ?? r.commit_type }}
            </n-tag>
            <n-tag v-if="r.current" size="tiny" type="success">当前</n-tag>
            <span class="muted version-byline">
              {{ r.author }} · <code class="mono">{{ short(r.commit_id) }}</code>
              <template v-if="r.version_id"> · {{ (r.size_bytes / 1024).toFixed(1) }} KB</template>
            </span>
          </div>

          <div class="version-actions">
            <n-button
              size="tiny"
              quaternary
              :disabled="r.change_type === 'deleted'"
              @click.stop="openDiff(r)"
            >
              查看变更
            </n-button>
            <n-button
              v-if="store.canWrite && !r.current"
              size="tiny"
              type="primary"
              ghost
              :disabled="r.change_type === 'deleted' || reverting"
              @click.stop="confirmRevert(r)"
            >
              回溯到此版本
            </n-button>
          </div>
        </div>
      </n-spin>
    </aside>

    <n-modal
      v-model:show="showDiff"
      preset="card"
      :title="`变更 · ${docPath}`"
      style="width: 1000px; max-width: 94vw"
      :bordered="false"
    >
      <div class="row" style="margin-bottom: 10px">
        <span class="muted" style="font-size: 13px">对比</span>
        <n-select
          v-model:value="diffBaseId"
          size="small"
          style="width: 340px"
          :options="baseOptions"
          placeholder="与当前分支内容对比"
          clearable
        />
        <span class="muted" style="font-size: 13px">→</span>
        <n-tag size="small" type="info">{{ short(diffTarget?.commit_id) }}</n-tag>
      </div>

      <n-spin :show="diffLoading">
        <n-alert v-if="diff && !diff.has_changes" type="info" :show-icon="false">
          两侧内容一致，无差异。
        </n-alert>
        <div v-else-if="diff" class="card-surface" style="max-height: 60vh; overflow: auto">
          <div
            v-for="(row, i) in diffRows"
            :key="i"
            class="diff-line"
            :class="{ add: row.kind === 'add', del: row.kind === 'del', muted: row.kind === 'hunk' }"
            style="display: flex; white-space: pre"
          >
            <span class="diff-gutter">{{ row.oldNo ?? '' }}</span>
            <span class="diff-gutter">{{ row.newNo ?? '' }}</span>
            <span class="diff-text">{{ row.kind === 'hunk' ? row.text : (row.kind === 'add' ? '+' : row.kind === 'del' ? '-' : ' ') + row.text }}</span>
          </div>
        </div>
      </n-spin>
    </n-modal>

    <!-- 右：选中版本的只读预览 -->
    <main class="reader-main">
      <template v-if="selected">
        <div
          style="
            padding: 8px 18px;
            background: var(--brand-soft);
            color: var(--brand-hover);
            font-size: 13px;
            border-bottom: 1px solid var(--rule);
          "
        >
          历史版本预览：{{ short(selected.commit_id) }} · {{ fmt(selected.created_at) }}（只读，未写回当前文档）
        </div>

        <n-spin :show="previewLoading">
          <div style="padding: 24px 32px">
            <p v-if="selected.change_type === 'deleted'" class="empty-state">
              该修订是删除操作，没有可预览的内容
            </p>
            <div v-else ref="previewBodyEl" class="markdown-body" v-html="previewHtml"></div>
          </div>
        </n-spin>
      </template>

      <p v-else class="empty-state">选择左侧任一版本查看预览</p>
    </main>

    <aside class="reader-toc">
      <DocOutline :host="previewBodyEl" :html="previewHtml" />
    </aside>
  </div>
</template>

<style scoped>
/* 左侧版本列表在 272px 的侧栏里，尽量压紧每一条 */
.reader-aside :deep(.version-item) {
  padding: 7px 9px;
  margin-bottom: 6px;
}
.version-line {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.version-msg {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.version-time {
  flex: none;
  font-size: 11.5px;
  color: var(--muted);
  white-space: nowrap;
}
.version-meta {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 4px;
  font-size: 11.5px;
}
.version-byline {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.version-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-top: 4px;
}

.diff-gutter {
  display: inline-block;
  width: 42px;
  min-width: 42px;
  padding: 0 6px;
  text-align: right;
  color: var(--muted);
  user-select: none;
  border-right: 1px solid var(--rule-soft);
}
.diff-text {
  padding-left: 8px;
  flex: 1;
}
</style>
