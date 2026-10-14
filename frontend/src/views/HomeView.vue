<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { NGrid, NGi, NInput, NButton, NSpace, NTag, NEmpty, NPagination, useMessage } from 'naive-ui'
import { getSiteSettings, listPublicProjects, type PublicProjectItem } from '@/api/public'

const router = useRouter()
const message = useMessage()
const projects = ref<PublicProjectItem[]>([])
const query = ref('')
const page = ref(1)
const total = ref(0)
const pageSize = 12
const loading = ref(false)
// 管理员可在管理界面改这个标题，默认「公开项目」
const homeTitle = ref('公开项目')

async function load() {
  loading.value = true
  try {
    const data = await listPublicProjects({
      query: query.value,
      page: page.value,
      pageSize,
    })
    projects.value = data.items
    total.value = data.total
  } catch (e: unknown) {
    message.error(String((e as Error)?.message ?? e))
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  load()
  // 标题取不到就保持默认值，不影响首页其它内容
  getSiteSettings()
    .then((s) => {
      if (s.home_title) homeTitle.value = s.home_title
    })
    .catch(() => {})
})

function search() {
  page.value = 1
  load()
}
</script>

<template>
  <div class="page">
    <section class="home-hero">
      <h1>{{ homeTitle }}</h1>
      <p>浏览已发布的 Markdown 文档集，文档间链接关系自动构建知识图谱</p>
      <n-space justify="center">
        <n-input
          v-model:value="query"
          class="search-input"
          placeholder="搜索项目..."
          @keyup.enter="search"
        />
        <n-button type="primary" @click="search">搜索</n-button>
      </n-space>
    </section>

    <div class="home-results">
      <n-empty
        v-if="!loading && projects.length === 0"
        class="empty-state"
        description="暂无公开项目"
      />

      <n-grid v-else :cols="3" :x-gap="16" :y-gap="16">
        <n-gi v-for="p in projects" :key="p.id">
          <div class="doc-card" @click="router.push(`/pub/${p.slug}/tree`)">
            <div class="row">
              <h4 class="grow ellipsis">{{ p.name }}</h4>
              <n-tag size="small" type="success">{{ p.doc_count }} 篇文档</n-tag>
            </div>
            <p>{{ p.description || '—' }}</p>
            <div class="meta">{{ p.owner_name }}</div>
          </div>
        </n-gi>
      </n-grid>
    </div>

    <n-space v-if="total > pageSize" class="home-pager" justify="center">
      <n-pagination v-model:page="page" :page-count="Math.ceil(total / pageSize)" @update:page="load" />
    </n-space>
  </div>
</template>

<style scoped>
.search-input {
  width: 320px;
}
.home-results {
  margin-top: 28px;
}
.home-pager {
  margin-top: 24px;
}
.doc-card {
  cursor: pointer;
}
.doc-card p {
  min-height: 22px;
}
</style>
