<script setup lang="ts">
import { NButton, NInput } from 'naive-ui'
import type { SearchHitDto } from '@/api/types'

defineProps<{
  term: string
  searching: boolean
  /** 是否已经检索过，用来区分「还没搜」和「没搜到」 */
  searched: boolean
  hits: SearchHitDto[]
  total: number
  truncated: boolean
  /** 当前选中/跳转过的结果下标 */
  activeIndex: number
}>()

const emit = defineEmits<{
  (e: 'update:term', value: string): void
  (e: 'search'): void
  (e: 'open', hit: SearchHitDto, index: number): void
}>()
</script>

<template>
  <div class="row" style="gap: 6px; margin-bottom: 8px">
    <n-input
      :value="term"
      size="small"
      placeholder="在全部文档里检索…"
      clearable
      style="flex: 1; min-width: 0"
      @update:value="emit('update:term', $event)"
      @keyup.enter="emit('search')"
    />
    <n-button size="small" type="primary" :loading="searching" @click="emit('search')">
      检索
    </n-button>
  </div>

  <p v-if="!searched" class="empty-state" style="padding: 20px 8px">
    输入关键词，在所有文档正文里检索
  </p>
  <p v-else-if="!hits.length" class="empty-state" style="padding: 20px 8px">
    没有找到「{{ term }}」
  </p>
  <template v-else>
    <p class="muted" style="font-size: 12px; padding: 0 8px 6px">
      共 {{ total }} 处命中{{ truncated ? `，仅显示前 ${hits.length} 处` : '' }}
    </p>
    <button
      v-for="(h, i) in hits"
      :key="`${h.doc_id}-${h.line}-${i}`"
      type="button"
      class="search-hit"
      :class="{ active: i === activeIndex }"
      @click="emit('open', h, i)"
    >
      <span class="row-between">
        <span>{{ h.path }}</span>
        <span class="num">L{{ h.line }}</span>
      </span>
      <span class="search-snippet">
        {{ h.before }}<b>{{ h.matched }}</b>{{ h.after }}
      </span>
    </button>
  </template>
</template>
