import { computed, nextTick, ref, watch, type Ref } from 'vue'
import { useRoute } from 'vue-router'
import { useMessage } from 'naive-ui'
import type { SearchHitDto, SearchResultDto } from '@/api/types'

/** 正文里被标记的命中词 */
const HIT_MARK = 'mark.reader-hit'

export interface DocSearchOptions {
  /** 当前分支名（空则用默认分支） */
  branch: () => string | undefined
  /** 渲染后的正文容器 */
  bodyEl: Ref<HTMLElement | null>
  /** 正文重新渲染的信号（传渲染后的 html 即可） */
  renderKey: () => string
  /** 当前文档路径，用于判断结果里的前后文能否直接用 */
  docPath: () => string
  /** 发起检索：工作台与公开站点用各自的接口 */
  fetchHits: (params: { q: string; branch?: string; limit?: number }) => Promise<SearchResultDto>
  /** 跳到某条命中（各页面自己拼路由，并把词与序号带在 query 上） */
  goToHit: (hit: SearchHitDto) => void
  /** 让左侧面板可见（各页面自己控制面板开合） */
  onOpenPanel: () => void
}

/**
 * 阅读页的全文检索：检索状态、结果跳转、命中的正文高亮。
 * 工作台阅读页与公开阅读页共用，只有接口和路由前缀不同。
 */
export function useDocSearch(opts: DocSearchOptions) {
  /** 左侧面板显示文档树还是检索结果 */
  const asideTab = ref<'tree' | 'search'>('tree')
  const searchTerm = ref('')
  const searching = ref(false)
  /** 是否已经检索过，用来区分「还没搜」和「没搜到」 */
  const searched = ref(false)
  const hits = ref<SearchHitDto[]>([])
  const total = ref(0)
  const truncated = ref(false)
  const activeHit = ref(-1)
  /** 刚点过的结果，用来在正文里精确定位（URL 里只带词与序号） */
  const pendingHit = ref<SearchHitDto | null>(null)

  const message = useMessage()
  const route = useRoute()

  async function runSearch() {
    const term = searchTerm.value.trim()
    if (!term) return
    searching.value = true
    try {
      const res = await opts.fetchHits({
        q: term,
        branch: opts.branch() || undefined,
        limit: 50,
      })
      hits.value = res.hits
      total.value = res.total
      truncated.value = res.truncated
      activeHit.value = -1
      searched.value = true
    } catch (e: unknown) {
      message.error((e as Error).message)
    } finally {
      searching.value = false
    }
  }

  /** 打开左侧检索面板 */
  function openSearch() {
    opts.onOpenPanel()
    asideTab.value = 'search'
  }

  function openHit(hit: SearchHitDto, index: number) {
    activeHit.value = index
    pendingHit.value = hit
    opts.goToHit(hit)
  }

  function clearMarks(root: HTMLElement) {
    root.querySelectorAll(HIT_MARK).forEach((el) => {
      const parent = el.parentNode
      if (!parent) return
      parent.replaceChild(document.createTextNode(el.textContent ?? ''), el)
      parent.normalize()
    })
  }

  /** 归一化上下文：抹掉空白与 markdown/表格符号，以抵消源码与渲染结果的差异 */
  function normCtx(s: string) {
    return s.replace(/[\s|`*_[\]()#>\-]/g, '').toLowerCase()
  }

  function commonPrefix(a: string, b: string) {
    let i = 0
    while (i < a.length && i < b.length && a[i] === b[i]) i += 1
    return i
  }

  function commonSuffix(a: string, b: string) {
    let i = 0
    while (i < a.length && i < b.length && a[a.length - 1 - i] === b[b.length - 1 - i]) i += 1
    return i
  }

  /**
   * 在渲染后的正文里定位命中处。
   * 源码里的链接地址、内联标记等不会出现在渲染结果里，序号会因此偏移，
   * 所以先用前后文比对打分，序号只作兜底和同分时的偏好。
   */
  function locateHit(root: HTMLElement, hit: SearchHitDto | null, term: string, nth: number) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
    const nodes: Text[] = []
    const starts: number[] = []
    let text = ''
    let node = walker.nextNode()
    while (node) {
      nodes.push(node as Text)
      starts.push(text.length)
      text += node.nodeValue ?? ''
      node = walker.nextNode()
    }

    const needle = (hit?.matched ?? term).toLowerCase()
    if (!needle) return null
    const lower = text.toLowerCase()
    const found: number[] = []
    for (let at = lower.indexOf(needle); at >= 0; at = lower.indexOf(needle, at + needle.length)) {
      found.push(at)
    }
    if (!found.length) return null

    const wantBefore = normCtx(hit?.before ?? '')
    const wantAfter = normCtx(hit?.after ?? '')
    let best = found[Math.min(nth, found.length) - 1]
    let bestScore = -1
    found.forEach((pos, i) => {
      const before = normCtx(text.slice(Math.max(0, pos - 60), pos))
      const after = normCtx(text.slice(pos + needle.length, pos + needle.length + 60))
      const score =
        commonSuffix(before, wantBefore) +
        commonPrefix(after, wantAfter) +
        (i === nth - 1 ? 0.5 : 0)
      if (score > bestScore) {
        bestScore = score
        best = pos
      }
    })

    let at = 0
    for (let i = 0; i < starts.length; i += 1) {
      if (starts[i] <= best) at = i
    }
    return { node: nodes[at], start: best - starts[at], length: needle.length }
  }

  /** 把命中处包成 <mark class="reader-hit"> 并滚到它 */
  function markHit(root: HTMLElement, hit: SearchHitDto | null, term: string, nth: number) {
    const pos = locateHit(root, hit, term, nth)
    if (!pos) return
    const range = document.createRange()
    range.setStart(pos.node, pos.start)
    range.setEnd(pos.node, pos.start + pos.length)
    const mark = document.createElement('mark')
    mark.className = 'reader-hit'
    range.surroundContents(mark)
    mark.scrollIntoView({ block: 'center' })
  }

  /** 结果里带过来的检索词 / 命中序号 */
  const hitTerm = computed(() => (typeof route.query.q === 'string' ? route.query.q : ''))
  const hitNth = computed(() => Number(route.query.hit) || 0)

  watch(
    [opts.renderKey, hitTerm, hitNth],
    async () => {
      const root = opts.bodyEl.value
      if (!root) return
      clearMarks(root)
      if (!hitTerm.value || hitNth.value <= 0) return
      await nextTick()
      // 只在目标就是当前文档时用结果里的前后文，其余情况按序号兜底
      const hit = pendingHit.value?.path === opts.docPath() ? pendingHit.value : null
      markHit(root, hit, hitTerm.value, hitNth.value)
    },
    { flush: 'post' },
  )

  return {
    asideTab,
    searchTerm,
    searching,
    searched,
    hits,
    total,
    truncated,
    activeHit,
    runSearch,
    openSearch,
    openHit,
  }
}
