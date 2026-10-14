import { nextTick } from 'vue'
import { useRouter } from 'vue-router'
import { useMessage } from 'naive-ui'
import { matchDocPath, resolveDocHref } from '@/utils/markdown'

export interface DocLinksOptions {
  /** 当前文档路径，相对链接按它所在目录解析 */
  docPath: () => string
  /** 当前分支里可见的全部文档路径 */
  allPaths: () => string[]
  /** 命中目标后要跳转的地址 */
  buildHref: (path: string, anchor: string | null) => string
  /** 目标不在可见范围内时的提示语 */
  missingText: (path: string) => string
}

/**
 * 正文里的链接点击处理：标准相对链接按当前文档所在目录解析，`[[wiki]]`
 * 链接按项目根解析（和后端的解析顺序一致）；命中就交给路由跳转，落空给提示。
 *
 * 工作台阅读页与公开阅读页共用，保证两边行为一致。
 */
export function useDocLinks(opts: DocLinksOptions) {
  const router = useRouter()
  const message = useMessage()

  /** 锚点 id 就是标题的 slug，等新文档渲染完再滚 */
  async function scrollToAnchor(anchor: string) {
    await nextTick()
    document.getElementById(anchor)?.scrollIntoView({ block: 'start' })
  }

  return function onBodyClick(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest('a')
    if (!a) return
    const href = a.getAttribute('href') ?? ''
    const isWiki = a.hasAttribute('data-wiki')
    // 外链交给浏览器自己处理
    if (href.startsWith('http://') || href.startsWith('https://') || href.startsWith('mailto:')) {
      return
    }
    e.preventDefault()

    const current = opts.docPath()
    const resolved = resolveDocHref(
      current,
      isWiki ? (a.getAttribute('data-wiki') ?? href) : href,
      isWiki,
    )
    if (!resolved) return

    const hit = matchDocPath(opts.allPaths(), resolved.path, isWiki)
    if (!hit) {
      message.warning(opts.missingText(resolved.path))
      return
    }
    void router.push(opts.buildHref(hit, resolved.anchor))
    // 文档内跳锚点不会让路由重新加载页面，需要自己滚过去
    if (resolved.anchor && hit === current) void scrollToAnchor(resolved.anchor)
  }
}
