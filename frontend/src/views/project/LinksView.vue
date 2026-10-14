<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NSpin, NTag, useMessage } from 'naive-ui'
import * as echarts from 'echarts'
import { getLinks } from '@/api/projects'
import { encodeDocPath } from '@/utils/markdown'
import type { LinkEdge, LinksDto } from '@/api/types'
import { useProjectStore } from '@/stores/project'
import { useThemeStore } from '@/stores/theme'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const store = useProjectStore()
const themeStore = useThemeStore()
const slug = computed(() => route.params.slug as string)

const loading = ref(false)
const data = ref<LinksDto | null>(null)
const chartHost = ref<HTMLElement | null>(null)
let chart: echarts.ECharts | null = null

const categories = ['文档', '孤立文档', '含未解析链接', '缺失目标']
const colors = ['#0969da', '#909399', '#f0a020', '#d03050']

async function load() {
  loading.value = true
  try {
    data.value = await getLinks(slug.value, store.branchName || undefined)
    await nextTick()
    render()
  } catch (e: unknown) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}
onMounted(load)
watch(() => store.branchName, load)
// canvas colours are read from the tokens, so redraw when the theme flips
watch(() => themeStore.dark, () => render())

function categoryOf(n: LinksDto['nodes'][number]): number {
  if (n.dangling > 0) return 2
  if (n.inn === 0 && n.out === 0) return 1
  return 0
}

function render() {
  if (!chartHost.value || !data.value) return
  if (!chart) {
    chart = echarts.init(chartHost.value)
    chart.on('click', (params) => {
      const docPath = (params.data as { docPath?: string } | undefined)?.docPath
      if (docPath) router.push(`/p/${slug.value}/view/${encodeDocPath(docPath)}`)
    })
  }

  const d = data.value
  const degree = new Map<string, number>()
  const missing = new Map<string, string>()
  const links: { source: string; target: string; lineStyle?: object }[] = []
  for (const e of d.edges) {
    if (e.target != null) {
      degree.set(e.source, (degree.get(e.source) ?? 0) + 1)
      degree.set(e.target, (degree.get(e.target) ?? 0) + 1)
      links.push({ source: e.source, target: e.target })
    } else {
      const key = `missing:${e.raw_target}`
      if (!missing.has(key)) missing.set(key, e.raw_target)
      links.push({ source: e.source, target: key, lineStyle: { type: 'dashed', color: '#d03050' } })
    }
  }

  // Distinct documents can share the same heading title (e.g. the same doc set
  // imported at two paths), which made them look like duplicated nodes: the
  // label only showed the title. Fall back to the path for those collisions.
  const titleCount = new Map<string, number>()
  for (const n of d.nodes) {
    const t = n.title || n.path
    titleCount.set(t, (titleCount.get(t) ?? 0) + 1)
  }
  const nodeLabel = (n: LinksDto['nodes'][number]) => {
    const t = n.title || n.path
    return (titleCount.get(t) ?? 0) > 1 ? `${t} · ${n.path}` : t
  }

  const nodes = [
    ...d.nodes.map((n) => ({
      id: n.id,
      name: n.id,
      labelText: nodeLabel(n),
      docPath: n.path,
      category: categoryOf(n),
      symbolSize: 16 + Math.min(28, (degree.get(n.id) ?? 0) * 5),
      value: (degree.get(n.id) ?? 0) + n.dangling,
    })),
    ...[...missing.entries()].map(([key, raw]) => ({
      id: key,
      name: key,
      labelText: raw,
      category: 3,
      symbol: 'diamond',
      symbolSize: 12,
      value: 0,
    })),
  ]

  const css = getComputedStyle(document.documentElement)
  const labelColor = css.getPropertyValue('--ink-2').trim()
  const edgeColor = css.getPropertyValue('--rule').trim()

  chart.setOption(
    {
      tooltip: {
        confine: true,
        formatter: (p: { data?: { labelText?: string; docPath?: string; value?: number } }) => {
          const dd = p.data
          if (!dd) return ''
          const sub = dd.docPath ?? '未解析的链接目标'
          return `<div style="max-width:320px;word-break:break-all"><b>${dd.labelText}</b><br/><span style="color:var(--muted)">${sub}</span></div>`
        },
      },
      legend: [{ data: categories, bottom: 0 }],
      series: [
        {
          type: 'graph',
          layout: 'force',
          force: { repulsion: 260, edgeLength: [50, 140], gravity: 0.08 },
          roam: true,
          draggable: true,
          edgeSymbol: ['none', 'arrow'],
          edgeSymbolSize: 7,
          label: {
            show: true,
            position: 'right',
            fontSize: 11,
            color: labelColor,
            formatter: (p: { data: { labelText?: string; name: string } }) =>
              p.data.labelText ?? p.data.name,
          },
          emphasis: { focus: 'adjacency', lineStyle: { width: 3 } },
          data: nodes,
          links,
          categories: categories.map((name) => ({ name })),
          color: colors,
          lineStyle: { color: edgeColor, width: 1.5, curveness: 0.08 },
        },
      ],
    },
    true,
  )
}

function onResize() {
  chart?.resize()
}
onMounted(() => window.addEventListener('resize', onResize))
onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
  chart?.dispose()
  chart = null
})

const danglingEdges = computed<LinkEdge[]>(() =>
  (data.value?.edges ?? []).filter((e) => e.target == null),
)

const pathById = computed(() => {
  const m = new Map<string, string>()
  for (const n of data.value?.nodes ?? []) m.set(n.id, n.path)
  return m
})
</script>

<template>
  <div class="page page-wide">
    <div class="row-between" style="margin-bottom: 14px">
      <div class="row">
        <span style="font-weight: 600; font-size: 15px">链接关系图</span>
        <n-tag size="small" :bordered="false">
          分支 {{ store.branchName }} · {{ data?.nodes.length ?? 0 }} 篇 · {{ data?.edges.length ?? 0 }} 条链接
        </n-tag>
      </div>
      <n-button size="small" @click="load">刷新</n-button>
    </div>

    <n-spin :show="loading">
      <div v-if="!loading && !data?.nodes.length" class="empty-state">
        此分支暂无文档，先去文档树创建或导入
        <div class="row" style="justify-content: center; margin-top: 12px">
          <n-button size="small" type="primary" @click="router.push(`/p/${slug}/tree`)">前往文档树</n-button>
        </div>
      </div>

      <div
        v-show="data?.nodes.length"
        ref="chartHost"
        class="card-surface"
        style="height: calc(100vh - 300px); min-height: 420px"
      ></div>
    </n-spin>

    <div v-if="danglingEdges.length" style="margin-top: 18px">
      <div style="font-weight: 600; font-size: 14px; margin-bottom: 8px; color: #d03050">
        未解析链接（{{ danglingEdges.length }} 条）
      </div>
      <div class="card-surface" style="overflow: hidden">
        <div
          v-for="(e, i) in danglingEdges"
          :key="i"
          class="row"
          style="gap: 12px; padding: 6px 14px; font-size: 13px; border-bottom: 1px solid var(--rule-soft)"
          :style="i % 2 ? 'background: var(--bg-soft)' : ''"
        >
          <span class="grow muted" style="word-break: break-all">
            {{ pathById.get(e.source) ?? e.source }}
          </span>
          <span class="muted">→</span>
          <span class="grow" style="color: #d03050; word-break: break-all">
            {{ e.raw_target }}
            <n-tag v-if="e.type === 'wiki'" size="tiny" :bordered="false" style="margin-left: 6px">wiki</n-tag>
          </span>
        </div>
      </div>
    </div>
  </div>
</template>
