/**
 * 文档树的构建：项目工作台与公开阅读页共用同一套逻辑，
 * 避免两边的分层规则出现分歧。
 */

export interface DocTreeNode<T> {
  key: string
  label: string
  isLeaf: boolean
  isDoc: boolean
  doc?: T
  children?: DocTreeNode<T>[]
}

/**
 * 文件名开头的序号段：`02-01-cdr_dict_gender.md` → `['02', '01']`。
 * 限定 2–3 位数字是为了不把 `2026-09-15.md` 这类日期误判成序号。
 */
const SEGMENT = /^(\d{2,3})-/

/** 文档在序号层级里的位置；没有序号前缀时是空数组 */
function docChainOf(path: string): string[] {
  const name = path.split('/').pop() || path
  const chain: string[] = []
  let rest = name
  for (;;) {
    const m = SEGMENT.exec(rest)
    if (!m) return chain
    chain.push(m[1])
    rest = rest.slice(m[0].length)
  }
}

/** 去掉已经用来分层的序号前缀：`02-01-x.md` → `x.md` */
function labelOf(path: string, chain: string[]): string {
  const name = path.split('/').pop() || path
  const consumed = chain.reduce((n, seg) => n + seg.length + 1, 0)
  return name.slice(consumed) || name
}

export function buildDocTree<T extends { id: string; path: string }>(docs: T[]): DocTreeNode<T>[] {
  interface FileEntry {
    doc: T
    chain: string[]
  }
  interface DirNode {
    name: string
    dirs: Map<string, DirNode>
    files: FileEntry[]
  }
  const root: DirNode = { name: '', dirs: new Map(), files: [] }
  for (const d of docs) {
    const parts = d.path.split('/').filter(Boolean)
    let cur = root
    // 除文件名的最后一段都当真实目录
    for (let i = 0; i < parts.length - 1; i++) {
      const seg = parts[i]
      let next = cur.dirs.get(seg)
      if (!next) {
        next = { name: seg, dirs: new Map(), files: [] }
        cur.dirs.set(seg, next)
      }
      cur = next
    }
    cur.files.push({ doc: d, chain: docChainOf(d.path) })
  }

  function docNode(doc: T, chain: string[], children?: DocTreeNode<T>[]): DocTreeNode<T> {
    return {
      key: doc.id,
      label: labelOf(doc.path, chain),
      isLeaf: !children?.length,
      isDoc: true,
      doc,
      children,
    }
  }

  /**
   * 把同一目录下的文档按序号层级摊开：文件名里有几段序号就往下分几层，
   * 每层的父节点由该层对应的那篇文档充当——`02-字典落地表建模明细.md` 就是
   * 所有 `02-*` 对象页的父节点，不再额外造以序号命名的目录。
   *
   * 只有在该层没有「恰好一篇」对应文档时才退化成序号目录（`01`），
   * 否则那些文档就没有可挂靠的父节点了。
   */
  function chainLevel(
    files: FileEntry[],
    depth: number,
    selfPath: string,
    seg?: string,
  ): DocTreeNode<T>[] {
    const own = files.filter((f) => f.chain.length === depth)
    const nextSegs = [
      ...new Set(files.filter((f) => f.chain.length > depth).map((f) => f.chain[depth])),
    ].sort()
    const childNodes = nextSegs.flatMap((next) =>
      chainLevel(
        files.filter((f) => f.chain.length > depth && f.chain[depth] === next),
        depth + 1,
        selfPath ? `${selfPath}/${next}` : next,
        next,
      ),
    )

    // 这一层正好有一篇文档：它既是文档，也是下一层的父节点
    if (own.length === 1) {
      return [docNode(own[0].doc, own[0].chain, childNodes.length ? childNodes : undefined)]
    }
    const out = own.map((f) => docNode(f.doc, f.chain))
    if (childNodes.length) {
      if (seg) {
        out.push({
          // 用 group: 前缀，避免与真实目录的 dir: key 撞车
          key: `group:${selfPath}`,
          label: seg,
          isLeaf: false,
          isDoc: false,
          children: childNodes,
        })
      } else {
        // 顶层没有序号可以命名，直接把下一层铺开
        out.push(...childNodes)
      }
    }
    return out
  }

  function toNodes(dir: DirNode, prefix: string): DocTreeNode<T>[] {
    const dirNodes: DocTreeNode<T>[] = [...dir.dirs.values()]
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((sub) => {
        const p = prefix ? `${prefix}/${sub.name}` : sub.name
        return {
          key: `dir:${p}`,
          label: sub.name,
          isLeaf: false,
          isDoc: false,
          children: toNodes(sub, p),
        }
      })
    // 没有序号（通常是总览 / 索引页）的文档排在最前，层级随后
    return [...chainLevel(dir.files, 0, prefix), ...dirNodes]
  }

  return toNodes(root, '')
}

/** 从树根到目标节点的所有祖先 key，用于把它展开到可见 */
export function ancestorKeysOf<T>(nodes: DocTreeNode<T>[], key: string): string[] {
  const found: string[] = []
  const walk = (list: DocTreeNode<T>[], trail: string[]): boolean => {
    for (const n of list) {
      if (n.key === key) {
        found.push(...trail)
        return true
      }
      if (n.children?.length && walk(n.children, [...trail, n.key])) return true
    }
    return false
  }
  walk(nodes, [])
  return [...new Set(found)]
}
