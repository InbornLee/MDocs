<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NIcon, NSpin, NTag, NTree, useMessage, type TreeOption } from 'naive-ui'
import {
  ArrowBackOutline,
  EyeOutline,
  FolderOpenOutline,
  ListOutline,
  SearchOutline,
} from '@vicons/ionicons5'
import { getPublicDocContent, searchPublicDocs, type PublicDocContent } from '@/api/public'
import { usePublicProjectStore, type TreeNode } from '@/stores/publicProject'
import { ancestorKeysOf } from '@/utils/docTree'
import DocOutline from '@/components/DocOutline.vue'
import DocSearchPanel from '@/components/DocSearchPanel.vue'
import { useDocSearch } from '@/composables/useDocSearch'
import { useDocLinks } from '@/composables/useDocLinks'
import { createMarkdownIt, encodeDocPath, renderMermaid } from '@/utils/markdown'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const store = usePublicProjectStore()

const slug = computed(() => route.params.slug as string)
const docPath = computed(() => {
  const m = route.params.pathMatch
  return Array.isArray(m) ? m.join('/') : (m ?? '')
})

// images are served from the project's attachment route, so the renderer needs
// the project id and the document path to resolve them
const md = createMarkdownIt(() => ({
  projectId: store.detail?.id,
  docPath: docPath.value,
}))
const doc = ref<PublicDocContent | null>(null)
const loading = ref(false)

const html = computed(() => (doc.value ? md.render(doc.value.content) : ''))

// ---- reading layout: document tree (left) + outline (right) ----
// Same behaviour as the workspace reader, and the same preference keys: the
// reading layout is one setting shared by both views.
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
 *  public header sit above it), and a single column is used when both side
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
        router.push(`/pub/${slug.value}/view/${encodeDocPath(node.doc.path)}`)
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
    if (!d) throw new Error(`文档 ${docPath.value} 未发布或不存在`)
    doc.value = await getPublicDocContent(slug.value, d.id, store.detail?.branch.name)
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
watch([docPath, () => store.detail?.branch.name], load)

function fmt(s: string) {
  return new Date(s).toLocaleString('zh-CN', { dateStyle: 'medium', timeStyle: 'short' })
}

/** 公开页是纯阅读态：回到工作台走顶部导航，这里只留「阅读」指示 */

// ---- 全文检索（公开站点只覆盖已发布文档）----
const search = useDocSearch({
  branch: () => store.detail?.branch.name,
  bodyEl,
  renderKey: () => html.value,
  docPath: () => docPath.value,
  fetchHits: (params) => searchPublicDocs(slug.value, params),
  goToHit: (hit) => {
    void router.push({
      path: `/pub/${slug.value}/view/${encodeDocPath(hit.path)}`,
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

/** Follow internal links inside the rendered markdown. */
const onBodyClick = useDocLinks({
  docPath: () => docPath.value,
  allPaths: () => store.docs.map((d) => d.path),
  buildHref: (path, anchor) =>
    `/pub/${slug.value}/view/${encodeDocPath(path)}${anchor ? '#' + anchor : ''}`,
  missingText: (path) => `链接目标 ${path} 未发布或不存在`,
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
        <p v-if="!store.docs.length" class="empty-state" style="padding: 20px 8px">尚未发布文档</p>
        <n-tree
          v-else
          v-model:expanded-keys="expandedKeys"
          v-model:selected-keys="selectedKeys"
          :data="treeData"
          :node-props="treeNodeProps"
          :default-expanded-keys="expandedKeys"
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
            <span class="muted" style="font-size: 12.5px">更新于 {{ fmt(doc.updated_at) }}</span>
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
            <n-button size="tiny" quaternary @click="router.push(`/pub/${slug}/tree`)">
              <template #icon>
                <n-icon :component="ArrowBackOutline" />
              </template>
              返回目录
            </n-button>
            <n-button size="tiny" quaternary @click="openSearch">
              <template #icon>
                <n-icon :component="SearchOutline" />
              </template>
              检索
            </n-button>
            <n-button size="tiny" type="primary">
              <template #icon>
                <n-icon :component="EyeOutline" />
              </template>
              阅读
            </n-button>
          </div>
        </div>
      </div>

      <div ref="scrollEl" class="doc-body">
        <n-spin :show="loading">
          <p v-if="!doc && !loading" class="empty-state">文档不存在或未发布</p>
          <template v-else-if="doc">
            <div v-if="doc.description || doc.tags?.length" class="doc-meta">
              <p v-if="doc.description" class="doc-desc">{{ doc.description }}</p>
              <div v-if="doc.tags?.length" class="doc-tags">
                <n-tag v-for="t in doc.tags" :key="t" size="small" round :bordered="false">{{ t }}</n-tag>
              </div>
            </div>
            <div ref="bodyEl" class="markdown-body" v-html="html" @click="onBodyClick"></div>
          </template>
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
.doc-meta {
  margin: 0 0 18px;
}
.doc-desc {
  margin: 0 0 8px;
  color: var(--ink-2);
  font-size: 13.5px;
  line-height: 1.7;
}
.doc-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
</style>
