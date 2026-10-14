<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'

interface OutlineItem {
  id: string
  text: string
  level: number
}

const props = defineProps<{
  /** rendered markdown container whose headings make up the outline */
  host: HTMLElement | null
  /** the rendered html; changing it rebuilds the outline */
  html: string
}>()

const items = ref<OutlineItem[]>([])
const activeId = ref('')

/** Nearest scrollable ancestor, or null when the page itself scrolls. */
function findScrollParent(el: HTMLElement | null): HTMLElement | null {
  let cur = el?.parentElement ?? null
  while (cur) {
    const overflowY = getComputedStyle(cur).overflowY
    if (/(auto|scroll)/.test(overflowY) && cur.scrollHeight > cur.clientHeight + 4) return cur
    cur = cur.parentElement
  }
  return null
}

// The reading pane scrolls on its own in the workspace, while the public site
// scrolls the window — follow whichever one applies.
let scrollTarget: HTMLElement | Window | null = null

/** True when the scroll target has reached its end. */
function atEnd(): boolean {
  if (scrollTarget instanceof HTMLElement) {
    return scrollTarget.scrollTop + scrollTarget.clientHeight >= scrollTarget.scrollHeight - 4
  }
  const doc = document.scrollingElement
  if (!doc) return false
  return window.scrollY + window.innerHeight >= doc.scrollHeight - 4
}

/** Highlight the last heading that has scrolled past the top of the pane. */
function onScroll() {
  const list = items.value
  if (!list.length) return
  const paneTop = scrollTarget instanceof HTMLElement ? scrollTarget.getBoundingClientRect().top : 0
  let current = list[0].id
  for (const item of list) {
    const el = document.getElementById(item.id)
    if (!el) continue
    if (el.getBoundingClientRect().top - paneTop <= 120) current = item.id
    else break
  }
  // the last section may never reach the top of the pane (nothing left to
  // scroll), so being at the end always counts as that section
  if (atEnd()) current = list[list.length - 1].id
  activeId.value = current
}

function unbindScroll() {
  scrollTarget?.removeEventListener('scroll', onScroll)
  scrollTarget = null
}

function rebuild() {
  const root = props.host
  const list: OutlineItem[] = []
  if (root) {
    root.querySelectorAll<HTMLElement>('h1, h2, h3, h4').forEach((el) => {
      if (!el.id) return
      list.push({
        id: el.id,
        text: (el.textContent ?? '').trim(),
        level: Number(el.tagName.slice(1)),
      })
    })
  }
  items.value = list
  activeId.value = list[0]?.id ?? ''
  unbindScroll()
  if (list.length) {
    scrollTarget = findScrollParent(root) ?? window
    scrollTarget.addEventListener('scroll', onScroll, { passive: true })
  }
}

function goTo(id: string) {
  const el = document.getElementById(id)
  if (!el) return
  activeId.value = id
  el.scrollIntoView({ behavior: 'smooth', block: 'start' })
}

watch(
  () => [props.html, props.host] as const,
  async () => {
    await nextTick()
    rebuild()
  },
  { immediate: true, flush: 'post' },
)

onBeforeUnmount(unbindScroll)
</script>

<template>
  <nav class="doc-outline">
    <div class="row-between" style="margin-bottom: 6px">
      <span class="section-title" style="margin: 0">大纲</span>
      <slot name="actions" />
    </div>

    <p v-if="!items.length" class="empty-state" style="padding: 20px 8px">本文档没有标题</p>
    <template v-else>
      <a
        v-for="item in items"
        :key="item.id"
        class="toc-link"
        :class="{ active: item.id === activeId, lv3: item.level >= 3 }"
        :href="`#${item.id}`"
        :title="item.text"
        @click.prevent="goTo(item.id)"
      >
        {{ item.text }}
      </a>
    </template>
  </nav>
</template>
