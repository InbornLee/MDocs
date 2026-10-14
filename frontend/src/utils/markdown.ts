import MarkdownIt from 'markdown-it'
import type { MarkdownIt as MarkdownItInstance } from 'markdown-it'
import hljs from 'highlight.js/lib/common'

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

/**
 * Obsidian-style wiki links: `[[path]]` and `[[path|label]]` render as
 * anchors carrying `data-wiki`, so the viewer can navigate them like normal
 * relative links. Kept in sync with the backend link extractor.
 */
function wikiLinkPlugin(md: MarkdownItInstance) {
  md.inline.ruler.before('link', 'wikilink', (state, silent) => {
    const start = state.pos
    if (state.src.charCodeAt(start) !== 0x5b /* [ */) return false
    if (state.src.charCodeAt(start + 1) !== 0x5b) return false
    const end = state.src.indexOf(']]', start + 2)
    if (end < 0) return false

    const inner = state.src.slice(start + 2, end)
    if (!inner || inner.includes('\n') || inner.includes('[') || inner.includes(']')) {
      return false
    }

    if (!silent) {
      const [targetRaw, labelRaw] = inner.split('|')
      const target = targetRaw.trim()
      if (!target) return false
      const label = (labelRaw ?? target).trim() || target
      const open = state.push('link_open', 'a', 1)
      open.attrs = [
        ['href', md.normalizeLink(target)],
        ['class', 'wiki-link'],
        ['data-wiki', target],
      ]
      const text = state.push('text', '', 0)
      text.content = label
      state.push('link_close', 'a', -1)
    }
    state.pos = end + 2
    return true
  })
}

/**
 * YAML front matter: the `---` … `---` block at the very top of a document
 * (title / doc_type / generated …) is file metadata, not body text, so it is
 * hidden when reading while the stored content (and the editor, and exports)
 * keep it untouched.
 *
 * Only a leading `---`, a block that actually looks like YAML (at least one
 * `key:`) and a short closing line count as front matter — a document that
 * merely starts with a thematic break must not lose the text up to the next
 * one.
 */
const FRONT_MATTER = /^---[ \t]*\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/
const FRONT_MATTER_KEY = /^[A-Za-z_][\w.-]*[ \t]*:/m
const FRONT_MATTER_MAX_LINES = 50

function stripFrontMatter(src: string): string {
  const m = FRONT_MATTER.exec(src)
  if (!m || !FRONT_MATTER_KEY.test(m[1])) return src
  if (m[1].split('\n').length > FRONT_MATTER_MAX_LINES) return src
  return src.slice(m[0].length)
}

function frontMatterPlugin(md: MarkdownItInstance) {
  md.core.ruler.before('block', 'front_matter', (state) => {
    state.src = stripFrontMatter(state.src)
    return true
  })
}

/**
 * Lowercased, punctuation-free form used for heading ids: word characters keep
 * their case folded, whitespace becomes a dash. CJK is preserved (ids are only
 * ever looked up with `getElementById`, never with a CSS selector).
 */
export function slugifyHeading(text: string): string {
  const s = text
    .trim()
    .toLowerCase()
    .replace(/\s+/g, '-')
    .replace(/[^\p{L}\p{N}_-]/gu, '')
    .replace(/-{2,}/g, '-')
    .replace(/^-+|-+$/g, '')
  return s || 'section'
}

function inlineText(
  token: { children?: { type: string; content: string }[] | null } | undefined,
): string {
  if (!token?.children) return ''
  let out = ''
  for (const child of token.children) {
    if (child.type === 'text' || child.type === 'code_inline') out += child.content
    else if (child.type === 'softbreak' || child.type === 'hardbreak') out += ' '
  }
  return out.trim()
}

/**
 * Give every heading a stable `id` so the reading view can build an outline,
 * jump to a section and honour `#anchor` links. Duplicates get a numeric suffix.
 */
function headingAnchorPlugin(md: MarkdownItInstance) {
  md.core.ruler.push('heading_anchor', (state) => {
    const seen = new Map<string, number>()
    for (let i = 0; i < state.tokens.length; i++) {
      const open = state.tokens[i]
      if (open.type !== 'heading_open') continue
      const base = slugifyHeading(inlineText(state.tokens[i + 1]))
      const n = seen.get(base) ?? 0
      seen.set(base, n + 1)
      open.attrSet('id', n === 0 ? base : `${base}-${n}`)
    }
    return true
  })
}

/**
 * Wrap every table in `.table-wrap` so wide tables scroll horizontally while
 * the header row stays put (styles/global.css).
 */
function tableWrapPlugin(md: MarkdownItInstance) {
  md.renderer.rules.table_open = (tokens, idx, options, _env, self) =>
    `<div class="table-wrap">${self.renderToken(tokens, idx, options)}`
  md.renderer.rules.table_close = (tokens, idx, options, _env, self) =>
    `${self.renderToken(tokens, idx, options)}</div>`
}

/**
 * ```mermaid fenced blocks are drawn by mermaid in the browser: the rule emits
 * the mount point (`.mermaid`, text escaped so the source survives `v-html`)
 * and `renderMermaid` turns it into an SVG once the view has mounted.
 *
 * mermaid reads the element's innerHTML and then replaces it with the drawing,
 * so `data-mermaid-source` keeps the definition around for redraws.
 */
function mermaidPlugin(md: MarkdownItInstance) {
  const fence = md.renderer.rules.fence
  md.renderer.rules.fence = (tokens, idx, options, env, self) => {
    const token = tokens[idx]
    const info = token.info.trim().split(/\s+/)[0].toLowerCase()
    if (info === 'mermaid') {
      const source = escapeHtml(token.content)
      return `<div class="mermaid" data-mermaid-source="${source}">${source}</div>\n`
    }
    return fence
      ? fence(tokens, idx, options, env, self)
      : self.renderToken(tokens, idx, options)
  }
}

type MermaidApi = typeof import('mermaid')['default']

let mermaidApi: Promise<MermaidApi> | null = null

/** The diagram palette follows the workbench theme (a class on `<html>`). */
function mermaidTheme(): 'dark' | 'default' {
  return document.documentElement.classList.contains('dark') ? 'dark' : 'default'
}

function configureMermaid(api: MermaidApi) {
  api.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    theme: mermaidTheme(),
    // the diagram itself stays transparent so the surrounding block
    // (`.mermaid`, global.css) provides the background
    themeVariables: { background: 'transparent' },
    fontFamily: 'inherit',
  })
}

/** mermaid is a large bundle, so it only loads for a document that has a diagram. */
function loadMermaid(): Promise<MermaidApi> {
  mermaidApi ??= import('mermaid').then((m) => {
    configureMermaid(m.default)
    return m.default
  })
  return mermaidApi
}

/** Draw every not-yet-processed mermaid block inside `root`. */
export async function renderMermaid(root: HTMLElement | null | undefined): Promise<void> {
  const nodes = root?.querySelectorAll<HTMLElement>('.mermaid:not([data-processed])')
  if (!nodes || !nodes.length) return
  try {
    const api = await loadMermaid()
    await api.run({ nodes, suppressErrors: true })
  } catch (e: unknown) {
    console.error('[mermaid] 渲染失败', e)
  }
}

/**
 * Redraw the diagrams currently on screen. mermaid bakes the palette into the
 * generated SVG, so switching the workbench theme means rendering them again
 * from the source kept in `data-mermaid-source`.
 */
export async function refreshMermaidTheme(): Promise<void> {
  const nodes = Array.from(document.querySelectorAll<HTMLElement>('.mermaid[data-mermaid-source]'))
  if (!nodes.length) return
  try {
    const api = await loadMermaid()
    configureMermaid(api)
    for (const node of nodes) {
      node.innerHTML = escapeHtml(node.dataset.mermaidSource ?? '')
      node.removeAttribute('data-processed')
    }
    await api.run({ nodes, suppressErrors: true })
  } catch (e: unknown) {
    console.error('[mermaid] 重绘失败', e)
  }
}

/** Where the images of the document being rendered live. */
export interface MarkdownAssetContext {
  /** project id, used to build the attachment URL */
  projectId?: string
  /** project-relative path of the document being rendered */
  docPath?: string
}

/**
 * Resolve an image source against the document's directory, yielding a
 * project-relative path. Returns null for anything that is not a local file
 * (absolute URL, data URI, protocol-relative, anchor-only), so those keep the
 * source they were written with.
 */
function resolveAssetPath(fromPath: string, src: string): string | null {
  const raw = decodeHref(src.trim())
  if (!raw) return null
  if (/^[a-z][a-z0-9+.-]*:/i.test(raw) || raw.startsWith('//') || raw.startsWith('#')) {
    return null
  }
  // a leading '/' means the project root: attachments are stored relative to it
  const segs = raw.startsWith('/') ? [] : dirOf(fromPath).split('/').filter(Boolean)
  for (const seg of raw.split('/')) {
    if (!seg || seg === '.') continue
    if (seg === '..') segs.pop()
    else segs.push(seg)
  }
  return segs.length ? segs.join('/') : null
}

/**
 * Point image sources at the attachment route. Images arrive with an import
 * next to the documents (e.g. `../../assets/x.png`) and are served from
 * `/uploads/attachments/<project id>/<path>`, so a markdown reference has to be
 * resolved against the document's directory and rewritten.
 */
function assetPlugin(md: MarkdownItInstance, getCtx?: () => MarkdownAssetContext) {
  if (!getCtx) return
  const base = md.renderer.rules.image
  md.renderer.rules.image = (tokens, idx, options, env, self) => {
    const ctx = getCtx()
    const token = tokens[idx]
    const i = token.attrIndex('src')
    if (i >= 0 && ctx.projectId && token.attrs) {
      const rel = resolveAssetPath(ctx.docPath ?? '', String(token.attrs[i][1]))
      if (rel) {
        const encoded = rel.split('/').map(encodeURIComponent).join('/')
        token.attrs[i][1] = `/uploads/attachments/${ctx.projectId}/${encoded}`
      }
    }
    return base
      ? base(tokens, idx, options, env, self)
      : self.renderToken(tokens, idx, options)
  }
}

export function createMarkdownIt(
  assetContext?: () => MarkdownAssetContext,
): MarkdownItInstance {
  const md = new MarkdownIt({
    html: false,
    linkify: true,
    typographer: true,
    highlight(code: string, lang: string): string {
      if (lang && hljs.getLanguage(lang)) {
        try {
          return `<pre class="hljs"><code>${hljs.highlight(code, { language: lang }).value}</code></pre>`
        } catch {
          /* ignore */
        }
      }
      return `<pre class="hljs"><code>${escapeHtml(code)}</code></pre>`
    },
  })
  md.use(frontMatterPlugin)
  md.use(wikiLinkPlugin)
  md.use(headingAnchorPlugin)
  md.use(tableWrapPlugin)
  md.use(mermaidPlugin)
  if (assetContext) md.use(assetPlugin, assetContext)
  return md
}

const MD_EXT = /\.(md|markdown)$/i

/**
 * Encode a document path for use inside a router URL.
 *
 * A raw path cannot be interpolated straight into a `router.push` target:
 * `#` starts the URL fragment (so `My Note #1.md` would be truncated to
 * `My Note ` and the document lookup would fail), spaces/CJK get mangled and
 * `%` can produce invalid escapes. Encoding each segment — while keeping the
 * `/` separators intact — makes the round-trip lossless, since vue-router
 * decodes the captured `pathMatch` param back into the original path.
 */
export function encodeDocPath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/')
}

/** Drop `.md` / `.markdown` for the path used in viewer routes. */
export function normalizeDocPath(path: string): string {
  return path.replace(MD_EXT, '')
}

/** Directory part of a doc path (`""` when the doc sits at the root). */
export function dirOf(path: string): string {
  const i = path.lastIndexOf('/')
  return i < 0 ? '' : path.slice(0, i)
}

/**
 * markdown-it 渲染时会把链接目标里的非 ASCII 字符百分号编码
 * （`02_内镜EIS.md` → `02_%E5%86%85%E9%95%9CEIS.md`），这里还原回原始路径；
 * 非法转义（例如文件名里本来就有 `%`）解不开就原样返回。
 */
function decodeHref(s: string): string {
  try {
    return decodeURIComponent(s)
  } catch {
    return s
  }
}

/**
 * Resolve a markdown href from `fromPath` into a project-relative document
 * path. Standard links resolve against the source directory; wiki links
 * resolve from the project root (matching the backend's resolution order).
 * Returns null for external / anchor-only links.
 */
export function resolveDocHref(
  fromPath: string,
  href: string,
  isWiki = false,
): { path: string; anchor: string | null } | null {
  const raw = href.trim()
  if (!raw) return null
  if (/^[a-z][a-z0-9+.-]*:/i.test(raw) || raw.startsWith('//')) return null

  const hashAt = raw.indexOf('#')
  const anchor = hashAt >= 0 ? decodeHref(raw.slice(hashAt + 1)) : null
  const target = decodeHref((hashAt >= 0 ? raw.slice(0, hashAt) : raw).trim())
  if (!target) return { path: fromPath, anchor }

  const base = isWiki ? [] : dirOf(fromPath).split('/').filter(Boolean)
  for (const seg of target.replace(/^\.\//, '').replace(/^\//, '').split('/')) {
    if (!seg || seg === '.') continue
    if (seg === '..') base.pop()
    else base.push(seg)
  }
  return { path: base.join('/').replace(MD_EXT, ''), anchor }
}

/**
 * Map a resolved path onto an actual document path, tolerating links that
 * omit the extension and (for wiki links) bare file names.
 */
export function matchDocPath(
  allPaths: string[],
  resolved: string,
  isWiki = false,
): string | null {
  const norm = (p: string) => p.replace(MD_EXT, '')
  const want = norm(resolved)
  const exact = allPaths.find((p) => norm(p) === want)
  if (exact) return exact
  if (isWiki) {
    const base = want.split('/').pop() ?? want
    const candidates = allPaths.filter((p) => norm(p).split('/').pop() === base)
    if (candidates.length === 1) return candidates[0]
  }
  return null
}
