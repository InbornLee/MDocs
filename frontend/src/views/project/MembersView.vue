<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import {
  NAvatar,
  NButton,
  NDataTable,
  NModal,
  NSelect,
  NSpace,
  NTag,
  useDialog,
  useMessage,
} from 'naive-ui'
import type { DataTableColumns } from 'naive-ui'
import { useProjectStore } from '@/stores/project'
import {
  addMember,
  listMembers,
  removeMember,
  searchMemberCandidates,
  updateMember,
} from '@/api/projects'
import type { MemberCandidateDto, MemberDto } from '@/api/types'
import { ROLE_LABELS } from '@/utils/role'

const route = useRoute()
const message = useMessage()
const dialog = useDialog()
const store = useProjectStore()

const slug = computed(() => route.params.slug as string)
const isOwner = computed(() => store.project?.role === 'owner' || store.project?.is_owner === true)

const members = ref<MemberDto[]>([])
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    members.value = await listMembers(slug.value)
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(slug, load)

const roleOptions = [
  { label: ROLE_LABELS.owner, value: 'owner' },
  { label: ROLE_LABELS.editor, value: 'editor' },
  { label: ROLE_LABELS.reader, value: 'reader' },
]

const roleMeta: Record<string, { label: string; type: 'success' | 'info' | 'default' }> = {
  owner: { label: ROLE_LABELS.owner, type: 'success' },
  editor: { label: ROLE_LABELS.editor, type: 'info' },
  reader: { label: ROLE_LABELS.reader, type: 'default' },
}

function avatarUrl(file: string | null) {
  return file ? `/uploads/avatars/${file}` : undefined
}

const columns = computed<DataTableColumns<MemberDto>>(() => [
  {
    title: '成员',
    key: 'username',
    render: (row) => {
      const initial = (row.display_name || row.username).slice(0, 1).toUpperCase()
      return h('div', { class: 'row' }, [
        // n-avatar 只要拿到默认插槽内容就只渲染文字、忽略 src，所以有头像时不能给默认插槽
        h(NAvatar, { size: 26, src: avatarUrl(row.avatar_file) }, {
          default: row.avatar_file ? undefined : () => initial,
          fallback: () => initial,
        }),
        h('span', {}, row.display_name || row.username),
        h('span', { class: 'muted', style: 'font-size: 12px' }, `@${row.username}`),
      ])
    },
  },
  {
    title: '角色',
    key: 'role',
    width: 160,
    render: (row) => {
      if (!isOwner.value || row.is_owner) {
        const m = roleMeta[row.role] ?? { label: row.role, type: 'default' as const }
        return h(NTag, { size: 'small', type: m.type }, { default: () => m.label })
      }
      return h(NSelect, {
        value: row.role,
        size: 'tiny',
        options: roleOptions.filter((o) => o.value !== 'owner'),
        style: 'width:120px',
        'onUpdate:value': (v: string) => changeRole(row, v),
      })
    },
  },
  {
    title: '加入时间',
    key: 'created_at',
    width: 180,
    render: (row) => new Date(row.created_at).toLocaleString(),
  },
  {
    title: '',
    key: 'actions',
    width: 90,
    render: (row) =>
      isOwner.value && !row.is_owner
        ? h(
            NButton,
            { size: 'tiny', type: 'error', onClick: () => confirmRemove(row) },
            { default: () => '移除' },
          )
        : null,
  },
])

async function changeRole(row: MemberDto, role: string) {
  try {
    await updateMember(slug.value, row.user_id, role)
    message.success('角色已更新')
    await load()
  } catch (e: unknown) {
    message.error((e as Error).message)
  }
}

function confirmRemove(row: MemberDto) {
  dialog.warning({
    title: '移除成员',
    content: `确定将 @${row.username} 移出项目？`,
    positiveText: '移除',
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await removeMember(slug.value, row.user_id)
        message.success('已移除')
        await load()
      } catch (e: unknown) {
        message.error((e as Error).message)
      }
    },
  })
}

// ---- add member ----
const showAdd = ref(false)
const adding = ref(false)
// user 用 null 而不是 ''：naive-ui 的 select 把空串当成一个已选中的值，会吃掉 placeholder
const form = ref<{ user: string | null; role: string }>({ user: null, role: 'editor' })

const candidates = ref<MemberCandidateDto[]>([])
const searching = ref(false)
const SEARCH_DEBOUNCE = 250
let searchTimer: number | undefined
// 只认最后一次请求的结果，避免先发的慢响应盖掉后发的
let searchSeq = 0

const userOptions = computed(() => {
  const joined = new Set(members.value.map((m) => m.username))
  return candidates.value.map((u) => {
    const name = u.display_name && u.display_name !== u.username ? u.display_name : ''
    const label = name ? `${name}（@${u.username}）` : `@${u.username}`
    const inProject = joined.has(u.username)
    return {
      value: u.username,
      disabled: inProject,
      label: inProject ? `${label} 已在项目中` : label,
    }
  })
})

function onSearch(query: string) {
  window.clearTimeout(searchTimer)
  // 无论词是否为空都让在途请求失效，避免上次的结果盖回已清空的下拉框
  const seq = ++searchSeq
  const q = query.trim()
  if (!q) {
    candidates.value = []
    searching.value = false
    return
  }
  searching.value = true
  searchTimer = window.setTimeout(async () => {
    try {
      const rows = await searchMemberCandidates(slug.value, q)
      if (seq === searchSeq) candidates.value = rows
    } catch (e: unknown) {
      if (seq === searchSeq) message.error((e as Error).message)
    } finally {
      if (seq === searchSeq) searching.value = false
    }
  }, SEARCH_DEBOUNCE)
}

onBeforeUnmount(() => window.clearTimeout(searchTimer))

function openAdd() {
  form.value = { user: null, role: 'editor' }
  candidates.value = []
  searchSeq++
  searching.value = false
  showAdd.value = true
}

async function submitAdd() {
  const key = (form.value.user ?? '').trim()
  if (!key) {
    message.warning('请从下拉列表中选择用户')
    return
  }
  adding.value = true
  try {
    const m = await addMember(slug.value, { user: key, role: form.value.role })
    message.success(`已添加 @${m.username}`)
    showAdd.value = false
    await load()
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    adding.value = false
  }
}
</script>

<template>
  <div class="page">
    <section class="card-surface">
      <div class="row-between" style="padding: 12px 16px; border-bottom: 1px solid var(--rule-soft)">
        <span class="section-title" style="margin: 0">成员（{{ members.length }}）</span>
        <n-button v-if="isOwner" size="small" type="primary" @click="openAdd">添加成员</n-button>
      </div>

      <div style="padding: 12px 16px">
        <n-data-table
          v-if="members.length"
          :columns="columns"
          :data="members"
          :loading="loading"
          :bordered="false"
          size="small"
          :row-key="(r: MemberDto) => r.user_id"
        />
        <div v-else class="empty-state">暂无成员</div>
      </div>
    </section>

    <n-modal
      v-model:show="showAdd"
      preset="dialog"
      title="添加成员"
      positive-text="添加"
      negative-text="取消"
      :loading="adding"
      @positive-click="submitAdd"
    >
      <n-space vertical :size="12" style="margin-top: 8px">
        <n-select
          v-model:value="form.user"
          filterable
          remote
          clearable
          :options="userOptions"
          :loading="searching"
          placeholder="搜索用户名 / 昵称 / 邮箱"
          @search="onSearch"
        />
        <n-select
          v-model:value="form.role"
          :options="roleOptions.filter((o) => o.value !== 'owner')"
        />
      </n-space>
    </n-modal>
  </div>
</template>
