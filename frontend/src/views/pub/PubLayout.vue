<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NAlert, NButton, NSpin, NTag, NSelect, useMessage } from 'naive-ui'
import { usePublicProjectStore } from '@/stores/publicProject'
import { getToken } from '@/api/client'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const store = usePublicProjectStore()

const slug = computed(() => route.params.slug as string)
const branchName = ref<string>('')

const branchOptions = computed(() =>
  (store.detail?.branches ?? []).map((b) => ({
    label: b.is_default ? `${b.name}（默认）` : b.name,
    value: b.name,
  })),
)

async function reload() {
  await store.load(slug.value, branchName.value || undefined)
  if (store.error) message.error(store.error)
  if (store.detail) branchName.value = store.detail.branch.name
}

onMounted(reload)
watch(slug, () => {
  store.$reset()
  branchName.value = ''
  reload()
})
watch(branchName, (nb, ob) => {
  if (ob && nb !== ob) reload()
})

const navItems = computed(() => {
  const base = `/pub/${slug.value}`
  return [
    { label: '文档', to: `${base}/tree` },
    { label: '链接图', to: `${base}/links` },
  ]
})
</script>

<template>
  <div style="height: calc(100vh - 56px); display: flex; flex-direction: column">
    <div class="pub-header">
      <div class="row wrap" style="gap: 12px">
        <router-link to="/" class="brand" style="font-size: 19px">
          {{ store.detail?.name ?? slug }}
        </router-link>
        <n-tag size="small" type="success">公开</n-tag>
        <n-tag v-if="store.detail" size="small" :bordered="false">
          {{ store.detail.docs.length }} 篇文档
        </n-tag>
        <span v-if="store.detail?.owner_name" class="muted" style="font-size: 13px">
          by {{ store.detail.owner_name }}
        </span>
        <span v-if="store.detail?.description" class="muted" style="font-size: 13px">
          {{ store.detail.description }}
        </span>
      </div>

      <div class="row wrap" style="margin-top: 10px; gap: 14px">
        <n-select
          v-if="branchOptions.length"
          v-model:value="branchName"
          :options="branchOptions"
          size="small"
          style="width: 200px"
        />
        <nav class="nav-links" style="margin: 0; padding: 0; border: 0">
          <router-link v-for="item in navItems" :key="item.to" class="nav-link" :to="item.to">
            {{ item.label }}
          </router-link>
        </nav>
        <n-tag v-if="store.detail" size="small" :bordered="false" type="info">
          链接 {{ store.detail.link_summary.edges }} 条
          <template v-if="store.detail.link_summary.dangling">
            · 待补 {{ store.detail.link_summary.dangling }}
          </template>
        </n-tag>
        <n-button v-if="getToken()" size="tiny" @click="router.push('/projects')">
          进入工作台
        </n-button>
        <n-button v-else size="tiny" @click="router.push('/login')">登录</n-button>
      </div>
    </div>

    <n-alert v-if="store.error" type="error" style="margin: 20px 24px">
      {{ store.error }}
    </n-alert>

    <div v-else class="pub-body">
      <n-spin :show="store.loading" style="height: 100%">
        <router-view v-if="store.detail" />
      </n-spin>
    </div>
  </div>
</template>

<style scoped>
.pub-header {
  padding: 14px 24px;
  border-bottom: 1px solid var(--rule);
  background: var(--bg);
}
.pub-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
/* the reader inside fills the pane, so the spinner wrappers must not collapse */
.pub-body :deep(.n-spin-container),
.pub-body :deep(.n-spin-content) {
  height: 100%;
}
</style>
