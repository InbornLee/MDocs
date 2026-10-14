<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { Component } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import {
  NButton,
  NIcon,
  NInput,
  NTag,
  useDialog,
  useMessage,
} from 'naive-ui'
import {
  CreateOutline,
  EyeOutline,
  GridOutline,
  SaveOutline,
} from '@vicons/ionicons5'
import { EditorView, basicSetup } from 'codemirror'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { languages } from '@codemirror/language-data'
import { ApiError } from '@/api/client'
import { getDocContent, saveDoc } from '@/api/projects'
import { useProjectStore } from '@/stores/project'
import { createMarkdownIt, encodeDocPath, renderMermaid } from '@/utils/markdown'

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

const md = createMarkdownIt(() => ({
  projectId: store.project?.id,
  docPath: docPath.value,
}))

const editorHost = ref<HTMLElement | null>(null)
/** 预览窗格容器：```mermaid 代码块要等它挂载后才能画图 */
const previewEl = ref<HTMLElement | null>(null)
let view: EditorView | null = null

const loading = ref(false)
const saving = ref(false)
const docId = ref('')
const baseVersionId = ref('')
/** Must be reactive: `dirty` is a computed over it, and a plain variable left
 *  the status stuck on "未保存" after a successful save. */
const lastSavedContent = ref('')
const content = ref('')
const commitMsg = ref('')
type PaneMode = 'edit' | 'split' | 'preview'
const paneMode = ref<PaneMode>('split')
const conflict = ref(false)
let leaveConfirmed = false

/** 工具栏「编辑 / 阅读」切换：本页是编辑态，阅读按钮跳阅读路由 */
const mode = ref<'edit' | 'read'>('edit')

function switchMode(next: 'edit' | 'read') {
  if (next === 'read') {
    router.push(`/p/${slug.value}/view/${encodeDocPath(docPath.value)}`)
  }
}

const modeOptions: { label: string; value: PaneMode; icon: Component }[] = [
  { label: '编辑', value: 'edit', icon: CreateOutline },
  { label: '分屏', value: 'split', icon: GridOutline },
  { label: '预览', value: 'preview', icon: EyeOutline },
]

const dirty = computed(() => content.value !== lastSavedContent.value)

// ---- preview (debounced render) ----
const previewHtml = ref('')
let previewTimer: ReturnType<typeof setTimeout> | null = null
function schedulePreview() {
  if (previewTimer) clearTimeout(previewTimer)
  previewTimer = setTimeout(() => {
    previewHtml.value = md.render(content.value)
  }, 150)
}
watch(content, schedulePreview)

// ```mermaid blocks are drawn once the preview is in the DOM
watch(previewHtml, async () => {
  await nextTick()
  await renderMermaid(previewEl.value)
})

// CodeMirror needs to re-measure when unhidden
watch(paneMode, () => {
  view?.requestMeasure()
})

async function load() {
  view?.destroy()
  view = null
  loading.value = true
  conflict.value = false
  docId.value = ''
  content.value = ''
  lastSavedContent.value = ''
  previewHtml.value = ''
  commitMsg.value = ''
  try {
    const d = store.docByPath(docPath.value)
    if (!d) throw new Error(`文档 ${docPath.value} 不在此分支中`)
    const res = await getDocContent(slug.value, d.id, store.branchName)
    docId.value = res.id
    baseVersionId.value = res.version_id
    lastSavedContent.value = res.content
    content.value = res.content
    previewHtml.value = md.render(res.content)
    await nextTick()
    if (editorHost.value) {
      view = new EditorView({
        doc: res.content,
        extensions: [
          basicSetup,
          markdown({ base: markdownLanguage, codeLanguages: languages }),
          EditorView.lineWrapping,
          EditorView.theme({
            '&': { height: '100%', fontSize: '14px' },
            '.cm-scroller': {
              overflow: 'auto',
              fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
            },
          }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) content.value = u.state.doc.toString()
          }),
        ],
        parent: editorHost.value,
      })
      view.focus()
    }
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch([docPath, () => store.branchName], load)

onBeforeUnmount(() => {
  view?.destroy()
  view = null
  if (previewTimer) clearTimeout(previewTimer)
  window.removeEventListener('keydown', onKeydown)
  window.onbeforeunload = null
})

// ---- save ----
function onKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && (e.key === 's' || e.key === 'S')) {
    e.preventDefault()
    doSave()
  }
}
onMounted(() => window.addEventListener('keydown', onKeydown))

window.onbeforeunload = (e: BeforeUnloadEvent) => {
  if (dirty.value) {
    e.preventDefault()
    e.returnValue = ''
  }
}

async function doSave() {
  if (!docId.value || saving.value) return
  if (conflict.value) {
    message.warning('文档已被他人修改，请先点击“重新加载最新版”')
    return
  }
  if (!dirty.value) {
    message.info('没有需要保存的修改')
    return
  }
  saving.value = true
  try {
    const res = await saveDoc(slug.value, docId.value, {
      content: content.value,
      message: commitMsg.value.trim() || undefined,
      branch: store.branchName,
      base_version_id: baseVersionId.value,
    })
    if (res.unchanged) {
      message.info('内容无变化，未产生新版本')
    } else {
      message.success('已保存，新版本已提交')
      if (res.version_id) baseVersionId.value = res.version_id
    }
    lastSavedContent.value = content.value
    commitMsg.value = ''
    // refresh doc list: title / updated_at may have changed, backend link tree is updated on save
    store.loadDocs()
  } catch (e: unknown) {
    if (e instanceof ApiError && e.status === 409) {
      conflict.value = true
      dialog.warning({
        title: '保存冲突',
        content:
          '该文档已被其他人修改并提交了新版本，无法直接覆盖。可选择重新加载最新内容（当前编辑内容将被丢弃），或留在编辑器中先将内容复制备份。',
        positiveText: '重新加载最新版',
        negativeText: '留在编辑器',
        onPositiveClick: () => {
          load()
        },
      })
    } else {
      message.error((e as Error).message)
    }
  } finally {
    saving.value = false
  }
}

onBeforeRouteLeave(() => {
  if (leaveConfirmed || !dirty.value) return true
  return new Promise<boolean>((resolve) => {
    dialog.warning({
      title: '有未保存的修改',
      content: '离开后未保存的修改将丢失，确定要离开吗？',
      positiveText: '离开',
      negativeText: '留下',
      onPositiveClick: () => {
        leaveConfirmed = true
        resolve(true)
      },
      onNegativeClick: () => resolve(false),
    })
  })
})
</script>

<template>
  <div style="height: 100%; display: flex; flex-direction: column; min-height: 0">
    <!-- toolbar -->
    <div
      class="row wrap"
      style="padding: 10px 24px; border-bottom: 1px solid var(--rule); background: var(--bg)"
    >
      <n-tag size="small" :bordered="false">{{ docPath }}</n-tag>
      <n-tag v-if="conflict" size="small" type="error">版本冲突</n-tag>
      <n-tag v-else-if="dirty" size="small" type="warning">未保存</n-tag>
      <n-tag v-else size="small" type="success" :bordered="false">已同步</n-tag>

      <div class="row">
        <n-button
          v-for="m in modeOptions"
          :key="m.value"
          size="tiny"
          :type="paneMode === m.value ? 'primary' : 'default'"
          :quaternary="paneMode !== m.value"
          @click="paneMode = m.value"
        >
          <template #icon>
            <n-icon :component="m.icon" />
          </template>
          {{ m.label }}
        </n-button>
      </div>

      <div class="grow"></div>

      <n-input
        v-model:value="commitMsg"
        size="small"
        placeholder="版本说明（可选）"
        style="width: 240px"
        @keydown.enter.prevent
      />
      <n-button
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
      <n-button
        size="tiny"
        type="primary"
        :loading="saving"
        :disabled="!docId"
        @click="doSave"
      >
        <template #icon>
          <n-icon :component="SaveOutline" />
        </template>
        保存 (Ctrl+S)
      </n-button>
    </div>

    <!-- body -->
    <div
      class="editor-grid"
      style="flex: 1; min-height: 0; overflow: hidden"
      :style="paneMode === 'split' ? undefined : 'grid-template-columns: minmax(0, 1fr)'"
    >
      <section v-show="paneMode !== 'preview'" class="editor-pane" style="position: relative">
        <div ref="editorHost" style="height: 100%"></div>
        <div
          v-if="loading"
          style="
            position: absolute;
            inset: 0;
            display: flex;
            align-items: center;
            justify-content: center;
            color: var(--muted);
            background: color-mix(in srgb, var(--bg) 72%, transparent);
          "
        >
          加载中...
        </div>
      </section>

      <section
        v-show="paneMode !== 'edit'"
        class="editor-pane"
        :style="paneMode === 'split' ? undefined : 'border-left: 0'"
      >
        <div class="editor-preview" style="padding: 24px 32px">
          <div ref="previewEl" class="markdown-body" v-html="previewHtml"></div>
        </div>
      </section>
    </div>
  </div>
</template>
