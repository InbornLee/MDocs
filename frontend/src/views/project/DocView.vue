<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NIcon, NSpin, NTag, NTree, useMessage, type TreeOption } from 'naive-ui'
import {
  ArrowBackOutline,
  CloudOfflineOutline,
  CloudUploadOutline,
  CreateOutline,
  DownloadOutline,
  EyeOutline,
  FolderOpenOutline,
  ListOutline,
  SearchOutline,
  TimeOutline,
} from '@vicons/ionicons5'
import type { DocContent } from '@/api/types'
import { getDocContent, searchDocs, setDocPublished } from '@/api/projects'
import { api } from '@/api/client'
import { useProjectStore, type TreeNode } from '@/stores/project'
import { ancestorKeysOf } from '@/utils/docTree'
import DocOutline from '@/components/DocOutline.vue'
import DocSearchPanel from '@/components/DocSearchPanel.vue'
import { useDocSearch } from '@/composables/useDocSearch'
import { useDocLinks } from '@/composables/useDocLinks'
import { createMarkdownIt, encodeDocPath, renderMermaid } from '@/utils/markdown'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const store = useProjectStore()

const slug = computed(() => route.params.slug as string)
const docPath = computed(() => {
  const m = route.params.pathMatch
  return Array.isArray(m) ? m.join('/') : (m ?? '')
})

// images are served from the project's attachment route, so the renderer needs
// the project id and the document path to resolve them
const md = createMarkdownIt(() => ({
  projectId: store.project?.id,
  docPath: docPath.value,
}))
const doc = ref<DocContent | null>(null)
const loading = ref(false)
const publishing = ref(false)

const html = computed(() => (doc.value ? md.render(doc.value.content) : ''))

/** publish flag comes from the doc list so it stays in sync across views */
const published = computed(() => store.docByPath(docPath.value)?.published ?? true)
const projectPublic = computed(() => store.project?.visibility === 'public')

// ---- reading layout: document tree (left) + outline (right) ----
// Both panels are pinned by default on a wide screen and can be hidden; the
// choice is remembered per browser. These keys are shared with the public
// reader (PubDocView), so one reading layout applies to both views.
const bodyEl = ref<HTMLElement | null>(null)
const treeHost = ref<HTMLElement | null>(null)
/** the scrolling pane (the document body), reset when another doc is opened */
const scrollEl = ref<HTMLElement | null>(null)

// ```mermaid blocks are drawn once the rendered body is in the DOM
watch([html, bodyEl], async () => {
  await nextTick()
  await renderMermaid(bodyEl.value)
})

function panelPreference(key: string, fallback: boolean) {
  const stored = localStorage.getItem(key)
  return stored === null ? fallback : stored === '1'
}

const TREE_PANEL_KEY = 'mdocs.reader.tree'
const OUTLINE_PANEL_KEY = 'mdocs.reader.outline'
const roomy = typeof window !== 'undefined' && window.innerWidth >= 1100
const showTree = ref(panelPreference(TREE_PANEL_KEY, roomy))
const showOutline = ref(panelPreference(OUTLINE_PANEL_KEY, roomy))

function setTree(open: boolean) {
  showTree.value = open
  localStorage.setItem(TREE_PANEL_KEY, open ? '1' : '0')
  if (open) void revealCurrent()
}

function setOutline(open: boolean) {
  showOutline.value = open
  localStorage.setItem(OUTLINE_PANEL_KEY, open ? '1' : '0')
}

/** The reader fills the layout area instead of the whole viewport (topbar and
 *  project header sit above it), and a single column is used when both side
 *  panels are hidden. */
const shellStyle = computed(() => {
  const style: Record<string, string> = { height: '100%' }
  if (!showTree.value && !showOutline.value) style.gridTemplateColumns = 'minmax(0, 1fr)'
  return style
})

const treeData = computed(() => store.tree as unknown as TreeOption[])
const expandedKeys = ref<string[]>([])
const selectedKeys = ref<string[]>([])

/** Open the folders leading to the current document so it is always visible. */
function expandToCurrent() {
  const d = store.docByPath(docPath.value)
  const keys: string[] = store.tree
    .filter((n) => n.children?.length)
    .map((n) => n.key)
  if (d) keys.push(...ancestorKeysOf(store.tree, d.id))
  expandedKeys.value = [...new Set([...expandedKeys.value, ...keys])]
}

async function revealCurrent() {
  await nextTick()
  treeHost.value?.querySelector('.n-tree-node--selected')?.scrollIntoView({ block: 'nearest' })
}

function syncSelection() {
  const d = store.docByPath(docPath.value)
  selectedKeys.value = d ? [d.id] : []
  expandToCurrent()
  void revealCurrent()
}

function treeNodeProps({ option }: { option: TreeOption }) {
  const node = option as unknown as TreeNode
  return {
    onClick(e: MouseEvent) {
      // 点展开箭头时不跳转，交给 n-tree 自己展开/折叠
      if ((e.target as HTMLElement | null)?.closest('.n-tree-node-switcher')) return
      if (node.isDoc && node.doc) {
        router.push(`/p/${slug.value}/view/${encodeDocPath(node.doc.path)}`)
      } else if (expandedKeys.value.includes(node.key)) {
        expandedKeys.value = expandedKeys.value.filter((k) => k !== node.key)
      } else {
        expandedKeys.value = [...expandedKeys.value, node.key]
      }
    },
  }
}

watch([docPath, () => store.docs], syncSelection, { immediate: true })

async function load() {
  loading.value = true
  doc.value = null
  try {
    const d = store.docByPath(docPath.value)
    if (!d) throw new Error(`文档 ${docPath.value} 不在此分支中`)
    doc.value = await getDocContent(slug.value, d.id, store.branchName)
    // the toolbar is fixed and the body scrolls: start a freshly opened document
    // at the top of that pane. A `#anchor` in the URL is left alone so deep
    // links still land correctly.
    if (!route.hash) {
      await nextTick()
      if (scrollEl.value) scrollEl.value.scrollTop = 0
      else window.scrollTo({ top: 0 })
    }
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}
onMounted(load)
watch([docPath, () => store.branchName], load)

function fmtDate(s: string) {
  return new Date(s).toLocaleString('zh-CN', { dateStyle: 'medium', timeStyle: 'short' })
}

async function togglePublish() {
  const d = store.docByPath(docPath.value)
  if (!d) return
  publishing.value = true
  try {
    const next = !published.value
    await setDocPublished(slug.value, d.id, next)
    await store.loadDocs()
    message.success(next ? '已发布到公开站点' : '已取消发布')
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    publishing.value = false
  }
}

async function downloadDoc() {
  if (!doc.value) return
  try {
    const b = store.branchName ? `?branch=${encodeURIComponent(store.branchName)}` : ''
    const resp = await api.getRaw(
      `/api/projects/${slug.value}/docs/${doc.value.id}/export${b}`,
    )
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`)
    const blob = await resp.blob()
    const cd = resp.headers.get('content-disposition') || ''
    const m = /filename\*=UTF-8''([^;]+)/.exec(cd)
    const filename = m
      ? decodeURIComponent(m[1])
      : doc.value.path.split('/').pop() || 'document.md'
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = filename
    a.click()
    URL.revokeObjectURL(url)
  } catch (e: unknown) {
    message.error((e as Error).message)
  }
}

/** 工具栏「编辑 / 阅读」切换：本页是阅读态，编辑按钮跳编辑路由 */
const mode = ref<'edit' | 'read'>('read')

function switchMode(next: 'edit' | 'read') {
  const p = encodeDocPath(docPath.value)
  if (next === 'edit') {
    if (store.canWrite) router.push(`/p/${slug.value}/edit/${p}`)
  } else {
    router.push(`/p/${slug.value}/view/${p}`)
  }
}

// ---- 全文检索（当前分支所有文档）----
const search = useDocSearch({
  branch: () => store.branchName,
  bodyEl,
  renderKey: () => html.value,
  docPath: () => docPath.value,
  fetchHits: (params) => searchDocs(slug.value, params),
  goToHit: (hit) => {
    void router.push({
      path: `/p/${slug.value}/view/${encodeDocPath(hit.path)}`,
      query: { q: search.searchTerm.value.trim(), hit: String(hit.nth) },
    })
  },
  onOpenPanel: () => setTree(true),
})
const {
  asideTab,
  searchTerm,
  searching,
  searched,
  hits,
  total,
  truncated,
  activeHit,
  runSearch,
  openSearch,
  openHit,
} = search

/** 正文里的内部链接：命中就切文档，`[[wiki]]` 与不带扩展名也认（和公开阅读页同一套） */
const onBodyClick = useDocLinks({
  docPath: () => docPath.value,
  allPaths: () => store.docs.map((d) => d.path),
  buildHref: (path, anchor) =>
    `/p/${slug.value}/view/${encodeDocPath(path)}${anchor ? '#' + anchor : ''}`,
  missingText: (path) => `链接目标 ${path} 不在当前分支中`,
})
</script>

<template>
  <div
    class="reader-shell"
    :class="{ 'no-aside': !showTree, 'no-toc': !showOutline }"
    :style="shellStyle"
  >
    <!-- left: document tree / 全文检索 -->
    <aside v-if="showTree" class="reader-aside">
      <div class="row-between" style="margin-bottom: 6px">
        <span class="section-title" style="margin: 0">
          {{ asideTab === 'search' ? '全文检索' : '文档树' }}
        </span>
        <n-button size="tiny" quaternary @click="setTree(false)">收起</n-button>
      </div>

      <div class="row" style="gap: 4px; margin-bottom: 8px">
        <n-button
          size="tiny"
          :type="asideTab === 'tree' ? 'primary' : 'default'"
          :quaternary="asideTab !== 'tree'"
          @click="asideTab = 'tree'"
        >
          文档树
        </n-button>
        <n-button
          size="tiny"
          :type="asideTab === 'search' ? 'primary' : 'default'"
          :quaternary="asideTab !== 'search'"
          @click="asideTab = 'search'"
        >
          检索
        </n-button>
      </div>

      <div v-if="asideTab === 'tree'" ref="treeHost">
        <n-tree
          v-model:expanded-keys="expandedKeys"
          v-model:selected-keys="selectedKeys"
          :data="treeData"
          :node-props="treeNodeProps"
          :cancelable="false"
          block-line
          selectable
        />
      </div>

      <template v-else>
        <DocSearchPanel
          v-model:term="searchTerm"
          :searching="searching"
          :searched="searched"
          :hits="hits"
          :total="total"
          :truncated="truncated"
          :active-index="activeHit"
          @search="runSearch"
          @open="openHit"
        />
      </template>
    </aside>

    <!-- center: the document -->
    <main class="reader-main">
      <!-- fixed toolbar (path + actions): only the body below scrolls -->
      <div v-if="doc" class="doc-head">
        <div class="doc-head-inner row-between wrap">
          <div class="row wrap">
            <n-tag size="small" :bordered="false">{{ doc.path }}</n-tag>
            <span class="muted" style="font-size: 12.5px">更新于 {{ fmtDate(doc.updated_at) }}</span>
            <n-tag v-if="projectPublic && published" size="small" type="success">已公开</n-tag>
            <n-tag v-else-if="projectPublic" size="small">未发布</n-tag>
          </div>
          <div class="row wrap">
            <n-button v-if="!showTree" size="tiny" quaternary @click="setTree(true)">
              <template #icon>
                <n-icon :component="FolderOpenOutline" />
              </template>
              文档树
            </n-button>
            <n-button v-if="!showOutline" size="tiny" quaternary @click="setOutline(true)">
              <template #icon>
                <n-icon :component="ListOutline" />
              </template>
              大纲
            </n-button>
            <n-button size="tiny" quaternary @click="openSearch">
              <template #icon>
                <n-icon :component="SearchOutline" />
              </template>
              检索
            </n-button>
            <n-button size="tiny" quaternary @click="router.push(`/p/${slug}/tree`)">
              <template #icon>
                <n-icon :component="ArrowBackOutline" />
              </template>
              返回文档树
            </n-button>
            <n-button
              size="tiny"
              quaternary
              @click="router.push(`/p/${slug}/history/${encodeDocPath(doc.path)}`)"
            >
              <template #icon>
                <n-icon :component="TimeOutline" />
              </template>
              历史
            </n-button>
            <n-button size="tiny" quaternary @click="downloadDoc">
              <template #icon>
                <n-icon :component="DownloadOutline" />
              </template>
              下载
            </n-button>
            <n-button
              v-if="store.canWrite"
              size="tiny"
              quaternary
              :loading="publishing"
              @click="togglePublish"
            >
              <template #icon>
                <n-icon :component="published ? CloudOfflineOutline : CloudUploadOutline" />
              </template>
              {{ published ? '取消发布' : '发布' }}
            </n-button>
            <n-button
              v-if="projectPublic && published"
              size="tiny"
              quaternary
              @click="router.push(`/pub/${slug}/view/${encodeDocPath(doc.path)}`)"
            >
              <template #icon>
                <n-icon :component="EyeOutline" />
              </template>
              公开预览
            </n-button>
            <n-button
              v-if="store.canWrite"
              size="tiny"
              :type="mode === 'edit' ? 'primary' : 'default'"
              :quaternary="mode !== 'edit'"
              @click="switchMode('edit')"
            >
              <template #icon>
                <n-icon :component="CreateOutline" />
              </template>
              编辑
            </n-button>
            <n-button
              size="tiny"
              :type="mode === 'read' ? 'primary' : 'default'"
              :quaternary="mode !== 'read'"
              @click="switchMode('read')"
            >
              <template #icon>
                <n-icon :component="EyeOutline" />
              </template>
              阅读
            </n-button>
          </div>
        </div>
      </div>

      <div ref="scrollEl" class="doc-body" @click="onBodyClick">
        <n-spin :show="loading">
          <div v-if="doc" ref="bodyEl" class="markdown-body" v-html="html"></div>
        </n-spin>
      </div>
    </main>

    <!-- right: outline -->
    <aside v-if="showOutline" class="reader-toc">
      <DocOutline :host="bodyEl" :html="html">
        <template #actions>
          <n-button size="tiny" quaternary @click="setOutline(false)">收起</n-button>
        </template>
      </DocOutline>
    </aside>
  </div>
</template>

<style scoped>
/* The toolbar is a fixed band and the document body is the only scroll area, so
   the document name and the actions never scroll out of view. */
.reader-main {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.doc-head {
  flex: none;
  background: var(--bg);
  border-bottom: 1px solid var(--rule-soft);
}
/* same column metrics as .doc-body so the toolbar lines up with the content */
.doc-head-inner {
  padding: 14px 40px;
}
.doc-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding-top: 22px;
}
</style>

