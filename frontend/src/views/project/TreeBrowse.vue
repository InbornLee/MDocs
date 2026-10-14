<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  NButton,
  NDropdown,
  NInput,
  NModal,
  NSelect,
  NSpace,
  NSpin,
  NTag,
  NTree,
  useMessage,
  type TreeOption,
} from 'naive-ui'
import { useProjectStore, type TreeNode } from '@/stores/project'
import { createDoc, deleteDoc, moveDoc } from '@/api/projects'
import { encodeDocPath } from '@/utils/markdown'
import { renderTreeLabel } from '@/utils/treeLabel'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const store = useProjectStore()
const slug = computed(() => route.params.slug as string)

const expandedKeys = ref<string[]>([])
const selectedKeys = ref<string[]>([])

const treeData = computed(() => store.tree as unknown as TreeOption[])

const defaultExpanded = computed(() => {
  // 展开所有带子节点的顶层节点（父节点可能是文档本身，不再只是目录）
  return store.tree.filter((n) => n.children?.length).map((n) => n.key)
})

function ensureExpanded() {
  if (expandedKeys.value.length === 0) expandedKeys.value = defaultExpanded.value
}
ensureExpanded()

function nodeProps({ option }: { option: TreeOption }) {
  const node = option as unknown as TreeNode
  return {
    onClick(e: MouseEvent) {
      // 点在展开箭头上时交给 n-tree 自己处理：否则这里会再切一次，
      // 目录看起来「点了没反应」，父节点是文档时还会顺带跳转
      if ((e.target as HTMLElement | null)?.closest('.n-tree-node-switcher')) return
      if (node.isDoc && node.doc) {
        router.push(`/p/${slug.value}/view/${encodeDocPath(node.doc.path)}`)
      } else if (!node.isDoc) {
        if (expandedKeys.value.includes(node.key)) {
          expandedKeys.value = expandedKeys.value.filter((k) => k !== node.key)
        } else {
          expandedKeys.value = [...expandedKeys.value, node.key]
        }
      }
    },
    onContextmenu(e: MouseEvent) {
      e.preventDefault()
      if (node.isDoc && node.doc && store.canWrite) {
        contextDoc.value = node.doc
        showContextMenu.value = true
        contextX.value = e.clientX
        contextY.value = e.clientY
      }
    },
  }
}

// ---- context menu ----
const showContextMenu = ref(false)
const contextX = ref(0)
const contextY = ref(0)
const contextDoc = ref<{ id: string; path: string } | null>(null)

const contextOptions = [
  { label: '查看', key: 'view' },
  { label: '编辑', key: 'edit' },
  { label: '重命名 / 移动', key: 'rename' },
  { label: '删除', key: 'delete' },
]

function onContext(key: string) {
  const doc = contextDoc.value
  showContextMenu.value = false
  if (!doc) return
  if (key === 'view') router.push(`/p/${slug.value}/view/${encodeDocPath(doc.path)}`)
  else if (key === 'edit') router.push(`/p/${slug.value}/edit/${encodeDocPath(doc.path)}`)
  else if (key === 'rename') {
    renameTarget.value = doc
    renameValue.value = doc.path
    showRename.value = true
  } else if (key === 'delete') {
    removeDoc(doc)
  }
}

async function removeDoc(doc: { id: string; path: string }) {
  try {
    await deleteDoc(slug.value, doc.id, store.branchName)
    message.success(`已删除 ${doc.path}`)
    store.loadDocs()
  } catch (e: unknown) {
    message.error((e as Error).message)
  }
}

// ---- rename ----
const showRename = ref(false)
const renameTarget = ref<{ id: string; path: string } | null>(null)
const renameValue = ref('')

async function submitRename() {
  if (!renameTarget.value) return
  const np = renameValue.value.trim()
  if (!np || np === renameTarget.value.path) {
    showRename.value = false
    return
  }
  const clash = nameConflicts(np, renameTarget.value.id)
  if (clash.length) {
    showDupDialog('rename', clash)
    return
  }
  try {
    await moveDoc(slug.value, renameTarget.value.id, {
      new_path: np,
      branch: store.branchName,
    })
    message.success('已重命名')
    showRename.value = false
    store.loadDocs()
  } catch (e: unknown) {
    message.error((e as Error).message)
  }
}

// ---- create doc ----
const showCreate = ref(false)
const creating = ref(false)
const createForm = ref({
  path: '',
  content: '# 新文档\n\n',
  template: 'blank',
})

const templates = [
  { label: '空白文档', value: 'blank' },
  { label: 'README 模板', value: 'readme' },
  { label: '技术设计模板', value: 'design' },
]

const templateContent: Record<string, string> = {
  blank: '# 新文档\n\n',
  readme: '# 项目名称\n\n## 简介\n\n## 快速开始\n\n## 目录\n\n',
  design:
    '# 设计：xxx\n\n## 背景\n\n## 目标\n\n## 方案\n\n### 架构\n\n### 数据模型\n\n## 风险\n\n',
}

function openCreate() {
  createForm.value = { path: '', content: templateContent.blank, template: 'blank' }
  showCreate.value = true
}

async function submitCreate() {
  const p = createForm.value.path.trim()
  if (!p) {
    message.warning('请输入文档路径')
    return
  }
  const clash = nameConflicts(p)
  if (clash.length) {
    showDupDialog('create', clash)
    return
  }
  creating.value = true
  try {
    const d = await createDoc(slug.value, {
      path: p,
      content: createForm.value.content,
      branch: store.branchName,
    })
    message.success(`已创建 ${d.path}`)
    showCreate.value = false
    await store.loadDocs()
    router.push(`/p/${slug.value}/edit/${encodeDocPath(d.path)}`)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    creating.value = false
  }
}

// ---- same-name detection ----
// A project keeps one document per name: creating or renaming onto an existing
// path — or onto a document with the same file name in another directory —
// would silently add a second copy, so it is reported instead of submitted.
const dupDialog = ref(false)
const dupItems = ref<{ id: string; path: string; title: string }[]>([])
const dupSource = ref<'create' | 'rename'>('create')

function nameConflicts(path: string, excludeId?: string) {
  const target = path.trim().replace(/^\.\//, '')
  if (!target) return []
  const name = target.split('/').pop() || target
  return store.docs
    .filter(
      (d) =>
        d.id !== excludeId &&
        (d.path === target || (d.path.split('/').pop() || d.path) === name),
    )
    .map((d) => ({ id: d.id, path: d.path, title: d.title }))
}

function showDupDialog(source: 'create' | 'rename', items: { id: string; path: string; title: string }[]) {
  dupSource.value = source
  dupItems.value = items
  dupDialog.value = true
}

/// Dismissing the prompt reopens the form it interrupted (the dialog preset
/// closes the previous modal on click), so the name can be corrected right away.
function resumeAfterDup() {
  dupDialog.value = false
  if (dupSource.value === 'create') showCreate.value = true
  else showRename.value = true
}

function openDupDoc(path: string) {
  dupDialog.value = false
  router.push(`/p/${slug.value}/view/${encodeDocPath(path)}`)
}
</script>

<template>
  <div class="page">
    <div class="row-between wrap" style="margin-bottom: 14px">
      <div class="row">
        <span class="section-title" style="margin: 0">文档树</span>
        <n-tag size="small" :bordered="false">
          分支 {{ store.branchName }} · {{ store.docs.length }} 篇
        </n-tag>
      </div>
      <n-button v-if="store.canWrite" type="primary" size="small" @click="openCreate">新建文档</n-button>
    </div>

    <n-spin :show="store.docsLoading">
      <div v-if="!store.docsLoading && store.docs.length === 0" class="empty-state">
        此分支暂无文档
        <div v-if="store.canWrite" style="margin-top: 12px">
          <n-button size="small" type="primary" @click="openCreate">创建第一篇文档</n-button>
        </div>
      </div>

      <n-tree
        v-else
        v-model:expanded-keys="expandedKeys"
        v-model:selected-keys="selectedKeys"
        :data="treeData"
        :node-props="nodeProps"
        :render-label="renderTreeLabel"
        block-line
        selectable
        :default-expanded-keys="defaultExpanded"
      />
    </n-spin>

    <n-dropdown
      trigger="manual"
      placement="bottom-start"
      :show="showContextMenu"
      :x="contextX"
      :y="contextY"
      :options="contextOptions"
      @select="onContext"
      @clickoutside="showContextMenu = false"
    />

    <n-modal
      v-model:show="showRename"
      preset="dialog"
      title="重命名 / 移动文档"
      positive-text="确定"
      negative-text="取消"
      @positive-click="submitRename"
    >
      <n-space vertical style="margin-top: 8px">
        <n-input v-model:value="renameValue" placeholder="新路径，如 docs/arch/new-name.md" />
      </n-space>
    </n-modal>

    <n-modal
      v-model:show="showCreate"
      preset="dialog"
      title="新建文档"
      positive-text="创建"
      negative-text="取消"
      :loading="creating"
      @positive-click="submitCreate"
    >
      <n-space vertical style="margin-top: 8px">
        <n-input v-model:value="createForm.path" placeholder="路径，如 docs/arch/overview.md（需以 .md 结尾）" />
        <n-select v-model:value="createForm.template" :options="templates" />
      </n-space>
    </n-modal>

    <n-modal
      v-model:show="dupDialog"
      preset="dialog"
      type="warning"
      title="已存在同名文档"
      positive-text="知道了"
      @positive-click="resumeAfterDup"
    >
      <div style="font-size: 13px; line-height: 1.8">
        项目内已有 {{ dupItems.length }} 篇同名文档（路径相同或文件名相同），为避免同名重复，请换一个名称后再提交：
      </div>
      <div class="card-surface" style="margin-top: 8px; max-height: 200px; overflow: auto">
        <div
          v-for="(d, i) in dupItems"
          :key="d.id"
          class="row"
          style="padding: 6px 10px; font-size: 13px"
          :style="i % 2 ? 'background: var(--bg-soft)' : ''"
        >
          <span class="grow" style="word-break: break-all">{{ d.path }}</span>
          <n-button size="tiny" quaternary type="primary" @click="openDupDoc(d.path)">查看</n-button>
        </div>
      </div>
    </n-modal>
  </div>
</template>
