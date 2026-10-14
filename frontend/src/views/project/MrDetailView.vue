<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  NAlert,
  NButton,
  NDescriptions,
  NDescriptionsItem,
  NInput,
  NList,
  NListItem,
  NModal,
  NSpace,
  NTag,
  NTimeline,
  NTimelineItem,
  useDialog,
  useMessage,
} from 'naive-ui'
import { addMrReview, closeMr, getMr, mergeMr } from '@/api/projects'
import type { MrDetail, MrFilePlan } from '@/api/types'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const dialog = useDialog()

const slug = computed(() => route.params.slug as string)
const mrId = computed(() => route.params.mrId as string)

const loading = ref(false)
const detail = ref<MrDetail | null>(null)

const changeMeta: Record<string, { label: string; type: 'success' | 'warning' | 'error' | 'info' }> = {
  added: { label: '新增', type: 'success' },
  modified: { label: '修改', type: 'warning' },
  deleted: { label: '删除', type: 'error' },
}

function fmt(ts: string) {
  return new Date(ts).toLocaleString()
}

async function load() {
  loading.value = true
  try {
    detail.value = await getMr(slug.value, mrId.value)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch([slug, mrId], load)

// ---------- conflict resolution state ----------
// docId -> resolved content; deletes holds docIds resolved as deletion
const resolutions = reactive<Record<string, string>>({})
const deletes = reactive<Record<string, boolean>>({})

const conflictFiles = computed<MrFilePlan[]>(
  () => detail.value?.plan.filter((p) => p.has_conflict) ?? [],
)

const otherFiles = computed<MrFilePlan[]>(
  () => detail.value?.plan.filter((p) => !p.has_conflict) ?? [],
)

function initResolutions(d: MrDetail) {
  for (const k of Object.keys(resolutions)) delete resolutions[k]
  for (const k of Object.keys(deletes)) delete deletes[k]
  for (const f of d.plan) {
    if (!f.has_conflict) continue
    resolutions[f.document_id] = f.merged_content ?? ''
    deletes[f.document_id] = false
  }
}

watch(detail, (d) => {
  if (d && d.status === 'open') initResolutions(d)
})

function useOurs(f: MrFilePlan) {
  resolutions[f.document_id] = f.ours_content ?? ''
  deletes[f.document_id] = false
}

function useTheirs(f: MrFilePlan) {
  resolutions[f.document_id] = f.theirs_content ?? ''
  deletes[f.document_id] = false
}

function toggleDelete(f: MrFilePlan) {
  deletes[f.document_id] = !deletes[f.document_id]
}

function hasMarkers(f: MrFilePlan) {
  const c = resolutions[f.document_id] ?? ''
  return c.includes('<<<<<<< ours') || c.includes('>>>>>>> theirs')
}

// ---------- actions ----------
const reviewComment = ref('')
const submittingReview = ref(false)

async function submitReview(action: 'approve' | 'reject' | 'comment') {
  submittingReview.value = true
  try {
    await addMrReview(slug.value, mrId.value, {
      action,
      comment: reviewComment.value.trim() || undefined,
    })
    message.success('已提交评审')
    reviewComment.value = ''
    await load()
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    submittingReview.value = false
  }
}

const merging = ref(false)
const showMergeConfirm = ref(false)

function openMerge() {
  const unresolved = conflictFiles.value.filter(
    (f) => !deletes[f.document_id] && hasMarkers(f),
  )
  if (unresolved.length) {
    message.warning(`还有 ${unresolved.length} 个冲突文件包含未处理的冲突标记`)
    return
  }
  showMergeConfirm.value = true
}

async function doMerge() {
  const res: Record<string, string | null> = {}
  const dels: string[] = []
  for (const f of conflictFiles.value) {
    if (deletes[f.document_id]) {
      dels.push(f.document_id)
    } else {
      res[f.document_id] = resolutions[f.document_id] ?? ''
    }
  }
  merging.value = true
  try {
    const r = await mergeMr(slug.value, mrId.value, {
      resolutions: res,
      deletes: dels,
    })
    message.success(`已合并 ${r.files} 个文件`)
    showMergeConfirm.value = false
    await load()
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    merging.value = false
  }
}

const closing = ref(false)
function confirmClose() {
  dialog.warning({
    title: '关闭合并请求',
    content: '关闭后不可再合并，确定继续？',
    positiveText: '关闭',
    negativeText: '取消',
    onPositiveClick: async () => {
      closing.value = true
      try {
        await closeMr(slug.value, mrId.value)
        message.success('已关闭')
        await load()
      } catch (e: unknown) {
        message.error((e as Error).message)
      } finally {
        closing.value = false
      }
    },
  })
}
</script>

<template>
  <div class="page page-wide">
    <div v-if="!detail && !loading" class="empty-state">合并请求不存在</div>
    <template v-else-if="detail">
      <section class="card-surface" style="margin-bottom: 16px">
        <div class="row-between wrap" style="padding: 12px 16px; border-bottom: 1px solid var(--rule-soft)">
          <div class="row wrap">
            <span style="font-size: 16px">{{ detail.title }}</span>
            <n-tag
              size="small"
              :type="detail.status === 'open' ? 'info' : detail.status === 'merged' ? 'success' : 'default'"
            >
              {{ detail.status === 'open' ? '进行中' : detail.status === 'merged' ? '已合并' : '已关闭' }}
            </n-tag>
            <n-tag v-if="detail.target_is_default" size="small" type="warning">目标为主分支</n-tag>
          </div>
          <n-button size="small" @click="router.push(`/p/${slug}/mrs`)">返回列表</n-button>
        </div>

        <div style="padding: 12px 16px">
          <n-descriptions :column="2" size="small" label-placement="left">
            <n-descriptions-item label="分支">
              {{ detail.source }} → {{ detail.target }}
            </n-descriptions-item>
            <n-descriptions-item label="提交人">{{ detail.created_by }}</n-descriptions-item>
            <n-descriptions-item label="创建时间">{{ fmt(detail.created_at) }}</n-descriptions-item>
            <n-descriptions-item label="合并时间">
              {{ detail.merged_at ? fmt(detail.merged_at) : '—' }}
            </n-descriptions-item>
          </n-descriptions>

          <p v-if="detail.description" class="muted" style="margin: 10px 0 0; white-space: pre-wrap">
            {{ detail.description }}
          </p>

          <n-alert
            v-if="detail.blocked_reason"
            type="warning"
            :show-icon="true"
            style="margin-top: 12px"
          >
            {{ detail.blocked_reason }}
          </n-alert>
        </div>
      </section>

      <!-- plan -->
      <section class="card-surface" style="margin-bottom: 16px">
        <div style="padding: 12px 16px 8px">
          <h3 class="section-title" style="margin: 0">变更文件</h3>
        </div>

        <div style="padding: 0 16px 16px">
          <n-alert v-if="!detail.plan.length" type="info" :show-icon="false">
            该请求暂无有效变更（可能源分支已无领先提交）。
          </n-alert>

          <template v-else>
            <div v-if="otherFiles.length" style="margin-bottom: 14px">
              <div class="section-title">自动合并（{{ otherFiles.length }}）</div>
              <n-list bordered>
                <n-list-item v-for="f in otherFiles" :key="f.document_id">
                  <div class="row">
                    <n-tag size="small" :type="changeMeta[f.change]?.type ?? 'default'">
                      {{ changeMeta[f.change]?.label ?? f.change }}
                    </n-tag>
                    <span class="mono" style="font-size: 13px">{{ f.path }}</span>
                  </div>
                </n-list-item>
              </n-list>
            </div>

            <div v-if="conflictFiles.length">
              <div class="section-title">存在冲突，需手动解决（{{ conflictFiles.length }}）</div>
              <div
                v-for="f in conflictFiles"
                :key="f.document_id"
                class="card-surface"
                style="margin-bottom: 12px; padding: 12px 16px"
              >
                <div class="row-between wrap" style="margin-bottom: 10px">
                  <div class="row wrap">
                    <n-tag size="small" type="error">
                      {{ f.conflict_kind === 'modify-delete' ? '修改/删除' : f.conflict_kind === 'add-add' ? '新增/新增' : '内容冲突' }}
                    </n-tag>
                    <span class="mono" style="font-size: 13px">{{ f.path }}</span>
                  </div>
                  <n-space :size="6">
                    <n-button size="tiny" @click="useOurs(f)">用目标分支</n-button>
                    <n-button size="tiny" @click="useTheirs(f)">用源分支</n-button>
                    <n-button
                      size="tiny"
                      :type="deletes[f.document_id] ? 'primary' : 'default'"
                      @click="toggleDelete(f)"
                    >
                      {{ deletes[f.document_id] ? '取消删除' : '标记删除' }}
                    </n-button>
                  </n-space>
                </div>

                <n-alert v-if="deletes[f.document_id]" type="error" :show-icon="false" style="margin-bottom: 8px">
                  合并时将删除该文档。
                </n-alert>

                <n-space v-else vertical :size="8">
                  <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 10px">
                    <div>
                      <div class="section-title">目标分支（当前）</div>
                      <n-input
                        :value="f.ours_content ?? '（无内容）'"
                        type="textarea"
                        :rows="6"
                        readonly
                        style="font-family: var(--mono)"
                      />
                    </div>
                    <div>
                      <div class="section-title">源分支（合入）</div>
                      <n-input
                        :value="f.theirs_content ?? '（无内容）'"
                        type="textarea"
                        :rows="6"
                        readonly
                        style="font-family: var(--mono)"
                      />
                    </div>
                  </div>
                  <div>
                    <div class="section-title">合并结果（可编辑；请移除冲突标记）</div>
                    <n-input
                      v-model:value="resolutions[f.document_id]"
                      type="textarea"
                      :rows="8"
                      style="font-family: var(--mono)"
                    />
                    <n-alert
                      v-if="hasMarkers(f)"
                      type="error"
                      :show-icon="false"
                      style="margin-top: 8px"
                    >
                      仍包含冲突标记 &lt;&lt;&lt;&lt;&lt;&lt;&lt; / &gt;&gt;&gt;&gt;&gt;&gt;&gt;
                    </n-alert>
                  </div>
                </n-space>
              </div>
            </div>
          </template>

          <template v-if="detail.status === 'open' && detail.plan.length">
            <n-space justify="end" style="margin-top: 8px">
              <n-button size="small" :disabled="!detail.can_merge" type="primary" @click="openMerge">
                合并到 {{ detail.target }}
              </n-button>
              <n-button size="small" @click="confirmClose" :loading="closing">关闭请求</n-button>
            </n-space>
          </template>
        </div>
      </section>

      <!-- reviews -->
      <section class="card-surface">
        <div style="padding: 12px 16px 8px">
          <h3 class="section-title" style="margin: 0">评审记录</h3>
        </div>

        <div style="padding: 0 16px 16px">
          <n-timeline v-if="detail.reviews.length">
            <n-timeline-item
              v-for="r in detail.reviews"
              :key="r.id"
              :type="r.action === 'approve' ? 'success' : r.action === 'reject' ? 'error' : 'info'"
              :title="`${r.reviewer} ${r.action === 'approve' ? '同意' : r.action === 'reject' ? '拒绝' : '评论'}`"
              :time="fmt(r.created_at)"
            >
              <span v-if="r.comment" style="white-space: pre-wrap">{{ r.comment }}</span>
            </n-timeline-item>
          </n-timeline>
          <div v-else class="empty-state" style="padding: 16px 0">暂无评审</div>

          <template v-if="detail.status === 'open'">
            <n-input
              v-model:value="reviewComment"
              type="textarea"
              :rows="2"
              placeholder="评审批注（可选）"
              style="margin-top: 10px"
            />
            <n-space justify="end" style="margin-top: 8px">
              <n-button size="small" :loading="submittingReview" @click="submitReview('comment')">
                评论
              </n-button>
              <n-button size="small" :loading="submittingReview" type="error" @click="submitReview('reject')">
                拒绝
              </n-button>
              <n-button size="small" :loading="submittingReview" type="primary" @click="submitReview('approve')">
                同意
              </n-button>
            </n-space>
          </template>
        </div>
      </section>
    </template>

    <n-modal
      v-model:show="showMergeConfirm"
      preset="dialog"
      type="warning"
      title="确认合并"
      :positive-text="merging ? '合并中…' : '确认合并'"
      negative-text="取消"
      :loading="merging"
      @positive-click="doMerge"
    >
      <div style="margin-top: 8px">
        将把 <b>{{ detail?.source }}</b> 的变更合并进 <b>{{ detail?.target }}</b>
        <span v-if="conflictFiles.length">，并应用 {{ conflictFiles.length }} 个冲突文件的解决方案</span>。
      </div>
    </n-modal>
  </div>
</template>
