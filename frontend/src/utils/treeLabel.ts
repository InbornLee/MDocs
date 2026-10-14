import { h } from 'vue'
import type { TreeOption } from 'naive-ui'

/** 文档树节点：目录节点只有 label，文档节点额外带一个 doc（含 updated_at） */
interface TimedNode {
  label: string
  isDoc: boolean
  doc?: { updated_at: string }
}

/** 具体时间，精确到分钟 */
function fmtTreeTime(ts: string) {
  return new Date(ts).toLocaleString('zh-CN', { dateStyle: 'short', timeStyle: 'short' })
}

/**
 * 相对时间（分钟 / 小时 / 天 / 周 / 月 / 年）。
 * 比一年还久的天数会落到「年前」，中间的月份是为了不出现「36 周前」这种写法。
 */
function relativeTime(ts: string) {
  const then = new Date(ts).getTime()
  if (Number.isNaN(then)) return ''
  const minutes = Math.floor(Math.max(0, Date.now() - then) / 60_000)
  if (minutes < 1) return '刚刚'
  if (minutes < 60) return `${minutes} 分钟前`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} 小时前`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days} 天前`
  if (days < 30) return `${Math.floor(days / 7)} 周前`
  if (days < 365) return `${Math.floor(days / 30)} 个月前`
  return `${Math.floor(days / 365)} 年前`
}

/** 「更新于 3 天前（2026/9/12 15:06）」 */
function updatedText(ts: string) {
  const abs = fmtTreeTime(ts)
  const rel = relativeTime(ts)
  return rel ? `更新于 ${rel}（${abs}）` : `更新于 ${abs}`
}

/**
 * n-tree 的 render-label：文档节点在名字右侧补一个「最后更新时间」，目录节点不显示。
 *
 * 样式只能内联——scoped 样式作用不到交给 NTree 渲染的 vnode 上。
 * 工作台文档树与公开阅读页的文档树共用这一份，保证两边显示一致。
 */
export function renderTreeLabel({ option }: { option: TreeOption }) {
  const node = option as unknown as TimedNode
  const doc = node.isDoc ? node.doc : null
  return h(
    'div',
    {
      style:
        'display:flex; align-items:baseline; justify-content:space-between;' +
        ' gap:12px; width:100%',
    },
    [
      h('span', {}, node.label),
      doc
        ? h(
            'span',
            {
              class: 'muted mono',
              style: 'flex:none; font-size:11.5px',
              title: new Date(doc.updated_at).toLocaleString('zh-CN'),
            },
            updatedText(doc.updated_at),
          )
        : null,
    ],
  )
}
