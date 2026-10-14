<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import {
  NButton,
  NGi,
  NGrid,
  NInput,
  NModal,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NSpace,
  NTag,
  NUpload,
  NUploadDragger,
  NAlert,
  NTimeline,
  NTimelineItem,
  useMessage,
  type UploadFileInfo,
} from 'naive-ui'
import { api } from '@/api/client'
import { useProjectStore } from '@/stores/project'

interface SkippedItem {
  path: string
  reason: string
}

/** An incoming file that already exists in the project. */
interface ImportConflict {
  path: string
  /** path = same path · name = same file name under another directory */
  kind: 'path' | 'name'
  existing_doc_id: string
  existing_path: string
  existing_title: string
}

interface MergedDoc {
  path: string
  has_conflict: boolean
}

interface ImportResult {
  branch: string
  imported: number
  /** 随导入一并写入的附件（图片等非 Markdown 文件）数量 */
  assets: number
  skipped: SkippedItem[]
  commit_id: string | null
  conflicts: ImportConflict[]
  merged: MergedDoc[]
  checked: boolean
}

const route = useRoute()
const message = useMessage()
const store = useProjectStore()
const slug = computed(() => route.params.slug as string)

// ---------- import (files / zip / directory) ----------

const importBranch = ref<string>('')
const importMessage = ref('')
const pickedFiles = ref<File[]>([])
const importing = ref(false)
const importResult = ref<ImportResult | null>(null)
const treeImportResult = ref<ImportResult | null>(null)

const branchOptions = computed(() =>
  store.branches.map((b) => ({
    label: b.is_default ? `${b.name}（默认）` : b.name,
    value: b.name,
  })),
)

function onFileListChange({ fileList }: { fileList: UploadFileInfo[] }) {
  const files: File[] = []
  for (const f of fileList) {
    if (f.status === 'removed') continue
    if (f.file) files.push(f.file)
  }
  pickedFiles.value = files
}

// ---------- duplicate handling ----------
// An import runs twice: first as a dry run (`mode = check`) so the server can
// tell us which incoming files already exist in the project, then for real with
// the mode the user picked (skip / overwrite / merge).

type ImportKind = 'files' | 'tree'

const showConflict = ref(false)
const conflictChoice = ref<'overwrite' | 'merge' | 'skip'>('overwrite')
const conflicts = ref<ImportConflict[]>([])
const pendingKind = ref<ImportKind>('files')
const treeDocs = ref<{ path: string; content: string }[]>([])

const samePathCount = computed(() => conflicts.value.filter((c) => c.kind === 'path').length)

function buildFileForm(mode: string): FormData {
  const form = new FormData()
  if (importBranch.value) form.append('branch', importBranch.value)
  form.append('mode', mode)
  if (importMessage.value.trim()) form.append('message', importMessage.value.trim())

  const plainFiles: File[] = []
  const plainPaths: string[] = []
  for (const f of pickedFiles.value) {
    if (f.name.toLowerCase().endsWith('.zip')) {
      // zip: single file part, no path mapping
      form.append('files', f, f.name)
      continue
    }
    plainFiles.push(f)
    // keep browser-relative path for directory picks, fallback to name
    const rel = (f as File & { webkitRelativePath?: string }).webkitRelativePath || ''
    plainPaths.push(rel && !rel.endsWith(f.name) ? rel : f.name)
  }
  if (plainFiles.length) {
    form.append('paths', JSON.stringify(plainPaths))
    for (const f of plainFiles) form.append('files', f, f.name)
  }
  return form
}

function postImport(kind: ImportKind, mode: string) {
  if (kind === 'files') {
    return api.post<ImportResult>(`/api/projects/${slug.value}/import`, buildFileForm(mode))
  }
  return api.post<ImportResult>(`/api/projects/${slug.value}/import/tree`, {
    branch: treeBranch.value || undefined,
    mode,
    message: importMessage.value.trim() || undefined,
    docs: treeDocs.value,
  })
}

/// Returns true when the import may proceed, false when the user still has to
/// decide how the existing documents of the same name are treated.
async function checkConflicts(kind: ImportKind): Promise<boolean> {
  importing.value = true
  try {
    const res = await postImport(kind, 'check')
    if (!res.conflicts.length) return true
    pendingKind.value = kind
    conflicts.value = res.conflicts
    conflictChoice.value = 'overwrite'
    showConflict.value = true
    return false
  } catch (e: unknown) {
    message.error((e as Error).message)
    return false
  } finally {
    importing.value = false
  }
}

async function runImport(kind: ImportKind, mode: string) {
  importing.value = true
  try {
    const res = await postImport(kind, mode)
    if (kind === 'files') importResult.value = res
    else treeImportResult.value = res
    await applyResult(res, mode)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    importing.value = false
  }
}

async function applyResult(res: ImportResult, mode: string) {
  if (res.imported > 0) {
    const applied =
      res.conflicts.length && mode === 'merge'
        ? `，其中 ${res.conflicts.length} 个同名文件已合并`
        : res.conflicts.length && mode === 'overwrite'
          ? `，其中 ${res.conflicts.length} 个同名文件已覆盖`
          : ''
    message.success(`已导入 ${res.imported} 篇文档${applied}`)
    await store.loadProject(slug.value, { keepBranch: true })
    await store.loadDocs(res.branch)
  } else if (res.conflicts.length) {
    message.info(`没有文档被导入：${res.conflicts.length} 个同名文件已跳过`)
  } else {
    message.info('没有新文档被导入')
  }
  const unresolved = res.merged.filter((m) => m.has_conflict)
  if (unresolved.length) {
    message.warning(
      `${unresolved.length} 篇文档合并时存在冲突：${unresolved
        .map((m) => m.path)
        .join('、')}，已写入 <<<<<<< / ======= / >>>>>>> 标记，请打开文档手工处理`,
    )
  }
}

function confirmConflict() {
  showConflict.value = false
  void runImport(pendingKind.value, conflictChoice.value)
}

function cancelConflict() {
  showConflict.value = false
  conflicts.value = []
  message.info('已取消导入')
}

async function submitImport() {
  if (!pickedFiles.value.length) {
    message.warning('请先选择文件')
    return
  }
  importResult.value = null
  if (!(await checkConflicts('files'))) return
  await runImport('files', 'skip')
}

// ---------- import (tree JSON) ----------

const treeJson = ref('')
const treeBranch = ref<string>('')

function onTreeFile(file: File) {
  const reader = new FileReader()
  reader.onload = () => {
    treeJson.value = String(reader.result || '')
  }
  reader.readAsText(file)
}

function onTreeFileChange({ fileList }: { fileList: UploadFileInfo[] }) {
  const f = fileList[fileList.length - 1]
  if (f?.file) onTreeFile(f.file)
}

async function submitTreeImport() {
  if (!treeJson.value.trim()) {
    message.warning('请先粘贴或选择 tree JSON')
    return
  }
  let docs: { path: string; content: string }[]
  try {
    const parsed = JSON.parse(treeJson.value)
    docs = parsed.docs ?? parsed
    if (!Array.isArray(docs)) throw new Error('bad format')
  } catch {
    message.error('不是有效的 tree JSON（需要 { docs: [{ path, content }] } 结构）')
    return
  }
  treeDocs.value = docs
  treeImportResult.value = null
  if (!(await checkConflicts('tree'))) return
  await runImport('tree', 'skip')
}

// ---------- export ----------

const exportBranch = ref<string>('')
const exporting = ref(false)

async function download(kind: 'zip' | 'tree') {
  exporting.value = true
  try {
    const b = exportBranch.value ? `?branch=${encodeURIComponent(exportBranch.value)}` : ''
    const resp = await api.getRaw(`/api/projects/${slug.value}/export/${kind}${b}`)
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`)
    const blob = await resp.blob()
    const cd = resp.headers.get('content-disposition') || ''
    const m = /filename\*=UTF-8''([^;]+)/.exec(cd)
    const filename = m ? decodeURIComponent(m[1]) : kind === 'zip' ? `${slug.value}.zip` : `${slug.value}-tree.json`
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = filename
    a.click()
    URL.revokeObjectURL(url)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    exporting.value = false
  }
}

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
</script>

<template>
  <div class="page">
    <n-grid :cols="2" :x-gap="16" :y-gap="16">
      <!-- import files -->
      <n-gi>
        <section class="card-surface" style="padding: 16px">
          <div class="section-title">导入文档</div>
          <n-space vertical :size="12">
            <n-alert type="info" :show-icon="false" style="font-size: 12.5px">
              支持多选 Markdown 文件、ZIP 压缩包或整个目录（目录导入请在选择文件时启用"文件夹"模式）。ZIP 中的 .md / .markdown 文件会导入为文档，图片等其它文件作为附件一并保存。
              导入前会检查项目内是否已有同名文档（路径相同或文件名相同），命中时弹窗让你选择覆盖 / 合并 / 跳过同名。
            </n-alert>

            <n-upload
              :default-upload="false"
              multiple
              directory-droppable
              @change="onFileListChange"
            >
              <n-upload-dragger>
                <div style="padding: 12px 0; font-size: 13.5px">
                  点击或拖拽文件到此处<br />
                  <span class="muted" style="font-size: 12px">.md / .markdown / .zip</span>
                </div>
              </n-upload-dragger>
            </n-upload>

            <n-select
              v-model:value="importBranch"
              :options="branchOptions"
              placeholder="导入到分支（默认主分支）"
              size="small"
            />
            <n-input v-model:value="importMessage" placeholder="提交说明（可选）" size="small" />
            <n-button type="primary" size="small" :loading="importing" :disabled="!pickedFiles.length" @click="submitImport">
              导入 {{ pickedFiles.length ? `(${pickedFiles.length} 个文件)` : '' }}
            </n-button>

            <template v-if="importResult">
              <n-alert :type="importResult.imported > 0 ? 'success' : 'warning'" :show-icon="false">
                分支 {{ importResult.branch }}：导入 {{ importResult.imported }} 篇
                <template v-if="importResult.skipped.length">
                  ，跳过 {{ importResult.skipped.length }} 项
                </template>
              </n-alert>
              <n-timeline v-if="importResult.skipped.length" style="margin-top: 4px">
                <n-timeline-item
                  v-for="s in importResult.skipped"
                  :key="s.path + s.reason"
                  type="warning"
                  :title="s.path"
                  :content="s.reason"
                />
              </n-timeline>
            </template>
          </n-space>
        </section>
      </n-gi>

      <!-- import tree json -->
      <n-gi>
        <section class="card-surface" style="padding: 16px">
          <div class="section-title">导入 tree JSON</div>
          <n-space vertical :size="12">
            <n-alert type="info" :show-icon="false" style="font-size: 12.5px">
              结构 <code>{ version, docs: [{ path, title, content }] }</code>，用于系统间迁移或程序化导入。
            </n-alert>
            <n-upload
              :default-upload="false"
              :max="1"
              accept=".json,application/json"
              @change="onTreeFileChange"
            >
              <n-button size="small">选择 JSON 文件</n-button>
            </n-upload>
            <n-input
              v-model:value="treeJson"
              type="textarea"
              placeholder='粘贴 tree JSON：{"docs":[{"path":"index.md","content":"# Hello"}]}'
              :rows="6"
            />
            <n-select
              v-model:value="treeBranch"
              :options="branchOptions"
              placeholder="导入到分支（默认主分支）"
              size="small"
            />
            <n-button type="primary" size="small" :loading="importing" :disabled="!treeJson.trim()" @click="submitTreeImport">
              导入 tree JSON
            </n-button>

            <template v-if="treeImportResult">
              <n-alert :type="treeImportResult.imported > 0 ? 'success' : 'warning'" :show-icon="false">
                分支 {{ treeImportResult.branch }}：导入 {{ treeImportResult.imported }} 篇
                <template v-if="treeImportResult.skipped.length">
                  ，跳过 {{ treeImportResult.skipped.length }} 项
                </template>
              </n-alert>
            </template>
          </n-space>
        </section>
      </n-gi>

      <!-- export -->
      <n-gi>
        <section class="card-surface" style="padding: 16px">
          <div class="section-title">导出文档</div>
          <n-space vertical :size="12">
            <n-alert type="info" :show-icon="false" style="font-size: 12.5px">
              按分支导出当前存活文档：ZIP 保留目录结构；tree JSON 为可再导入的完整快照；单篇导出请在文档阅读页使用下载按钮。
            </n-alert>
            <n-select
              v-model:value="exportBranch"
              :options="branchOptions"
              placeholder="导出分支（默认主分支）"
              size="small"
            />
            <n-space>
              <n-button size="small" type="primary" :loading="exporting" @click="download('zip')">
                导出 ZIP
              </n-button>
              <n-button size="small" :loading="exporting" @click="download('tree')">
                导出 tree JSON
              </n-button>
            </n-space>
          </n-space>
        </section>
      </n-gi>

      <!-- current state -->
      <n-gi>
        <section class="card-surface" style="padding: 16px">
          <div class="section-title">当前分支概览</div>
          <n-space vertical :size="8">
            <n-space align="center" :size="8">
              <n-tag size="small" type="info">分支：{{ store.branchName || 'main' }}</n-tag>
              <n-tag size="small">文档：{{ store.docs.length }} 篇</n-tag>
              <n-tag size="small">提交：{{ store.commitCount }} 个</n-tag>
            </n-space>
            <n-space align="center" :size="8" v-if="store.docs.length">
              <n-tag size="small" type="success">
                总体积 {{ fmtSize(store.docs.reduce((s, d) => s + (d.size_bytes || 0), 0)) }}
              </n-tag>
            </n-space>
          </n-space>
        </section>
      </n-gi>
    </n-grid>

    <n-modal
      v-model:show="showConflict"
      preset="card"
      title="检测到同名文档"
      style="max-width: 680px"
      :mask-closable="false"
    >
      <n-space vertical :size="12">
        <n-alert type="warning" :show-icon="false">
          本次导入有 {{ conflicts.length }} 个文件在项目内已存在同名文档
          <template v-if="samePathCount && samePathCount !== conflicts.length">
            （其中 {{ samePathCount }} 个路径完全相同）
          </template>
          ，请选择处理方式。
        </n-alert>

        <div class="card-surface" style="max-height: 260px; overflow: auto">
          <div
            v-for="(c, i) in conflicts"
            :key="c.path"
            class="row"
            style="gap: 10px; padding: 6px 10px; font-size: 13px"
            :style="i % 2 ? 'background: var(--bg-soft)' : ''"
          >
            <n-tag size="tiny" :bordered="false" :type="c.kind === 'path' ? 'warning' : 'info'">
              {{ c.kind === 'path' ? '路径相同' : '文件名相同' }}
            </n-tag>
            <span class="grow" style="word-break: break-all">{{ c.path }}</span>
            <span class="muted">→</span>
            <span class="grow muted" style="word-break: break-all">{{ c.existing_path }}</span>
          </div>
        </div>

        <n-radio-group v-model:value="conflictChoice" size="small">
          <n-space>
            <n-radio-button value="overwrite">覆盖</n-radio-button>
            <n-radio-button value="merge">合并</n-radio-button>
            <n-radio-button value="skip">跳过同名</n-radio-button>
          </n-space>
        </n-radio-group>

        <div class="muted" style="font-size: 12.5px; line-height: 1.8">
          <div><b>覆盖</b>：用导入内容替换已存在文档的内容，不会新增第二份。</div>
          <div>
            <b>合并</b>：以已存在文档的上一版本为基线做三段合并（ours = 现有内容，theirs = 导入内容），冲突处写入
            <code>&lt;&lt;&lt;&lt;&lt;&lt;&lt; ours / ======= / &gt;&gt;&gt;&gt;&gt;&gt;&gt; theirs</code>
            标记；该文档没有历史版本时等同覆盖。
          </div>
          <div><b>跳过同名</b>：只导入不冲突的文件。</div>
        </div>
      </n-space>

      <template #footer>
        <n-space justify="end">
          <n-button size="small" @click="cancelConflict">取消导入</n-button>
          <n-button size="small" type="primary" :loading="importing" @click="confirmConflict">
            确定
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>
