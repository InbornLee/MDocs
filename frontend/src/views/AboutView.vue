<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
// 作者与版本直接取 package.json，避免两处维护
import pkg from '../../package.json'
import { getSiteSettings } from '@/api/public'
import { changelog } from '@/content/changelog'

const author = pkg.author
const version = pkg.version
/** 第一条即当前版本；其余在「历史版本」里折叠展示 */
const latest = changelog[0]
const history = changelog.slice(1)

// 仓库地址与反馈入口由管理员在后台配置（站点设置），未配置就整块不显示
const repoUrl = ref('')
const feedbackUrl = ref('')
const feedbackEmail = ref('')

onMounted(async () => {
  try {
    const s = await getSiteSettings()
    repoUrl.value = s.repo_url ?? ''
    feedbackUrl.value = s.feedback_url ?? ''
    feedbackEmail.value = s.feedback_email ?? ''
  } catch {
    // 取不到站点配置时保持不显示，不影响页面其它内容
  }
})

const links = computed(() => {
  const out: { label: string; href: string; text: string; external: boolean }[] = []
  if (repoUrl.value) {
    out.push({ label: '代码仓库', href: repoUrl.value, text: repoUrl.value, external: true })
  }
  if (feedbackUrl.value) {
    out.push({ label: '问题反馈', href: feedbackUrl.value, text: feedbackUrl.value, external: true })
  }
  if (feedbackEmail.value) {
    out.push({
      label: '邮件反馈',
      href: `mailto:${feedbackEmail.value}`,
      text: feedbackEmail.value,
      external: false,
    })
  }
  return out
})

/** 每组：`[名称, 用途]` */
const backendStack: [string, string][] = [
  ['Rust 2021 + Axum 0.8', 'HTTP 服务与路由'],
  ['SeaORM 1.1', '实体与迁移，SQLite / PostgreSQL 双后端'],
  ['rmcp 3.2', 'Streamable HTTP MCP 服务端，供 AI 客户端接入'],
  ['rust-embed', '前端产物打进二进制，单个进程即可部署'],
  ['jsonwebtoken + argon2', '会话令牌与密码哈希'],
  ['comrak / zip / image', 'Markdown 解析、导入导出打包、头像处理'],
]

const frontendStack: [string, string][] = [
  ['Vue 3.5 + TypeScript + Vite 8', '单页应用骨架'],
  ['Naive UI 2.45', '组件库与中文文案'],
  ['Pinia + Vue Router', '状态管理与路由'],
  ['markdown-it 15 + highlight.js', '正文渲染与代码高亮'],
  ['CodeMirror 6', '在线文档编辑器'],
  ['ECharts 6', '链接关系等图表'],
]

const deployStack: [string, string][] = [
  ['Docker 单容器', 'SPA + REST API + MCP 挂在同一个进程，默认映射 8088 → 8080'],
  ['docker compose', 'SQLite 开箱即用，加 --profile postgres 可切换 PostgreSQL'],
  ['/data 卷', '数据库与上传头像，唯一有状态的目录'],
]

const features = [
  'Git 式的文档管理：分支、提交、合并请求、版本对比与回滚',
  '全项目跨文档全文检索，命中后可定位到正文对应位置',
  '公开站点：项目与单篇文档可分别发布，支持自定义首页标题',
  '成员与角色（拥有者 / 编辑者 / 只读），支持邀请与主动退出',
  'MCP 接口 + API token，让 AI 客户端以你的身份读写文档',
  '管理后台：站点设置、注册成员禁用启用与密码重置、公开项目治理',
]
</script>

<template>
  <div class="page about-page">
    <section class="card-surface about-hero">
      <h1 class="about-title">MDocs</h1>
      <p class="muted about-sub">Markdown 文档集管理系统 —— 把文档当代码那样管理</p>
      <div class="row wrap about-meta">
        <span>作者 <strong>{{ author }}</strong></span>
        <span class="dot">·</span>
        <span>版本 <code class="mono">v{{ version }}</code></span>
        <span class="dot">·</span>
        <span class="muted">© 2026</span>
      </div>
    </section>

    <section class="card-surface about-card">
      <div class="about-card-head">
        <div class="section-title">版本更新</div>
        <span class="muted about-card-note">最新 v{{ latest.version }}</span>
      </div>

      <article class="release">
        <header class="release-head">
          <span class="release-version mono">v{{ latest.version }}</span>
          <span class="release-badge">当前</span>
          <span class="muted release-date">{{ latest.date }}</span>
        </header>
        <p class="release-title">{{ latest.title }}</p>
        <div v-for="g in latest.groups" :key="g.kind" class="change-group">
          <span class="change-kind" :class="`kind-${g.kind}`">{{ g.label }}</span>
          <ul class="change-list">
            <li v-for="item in g.items" :key="item">{{ item }}</li>
          </ul>
        </div>
      </article>

      <details v-if="history.length" class="release-history">
        <summary>历史版本（{{ history.map((r) => 'v' + r.version).join('、') }}）</summary>
        <article v-for="r in history" :key="r.version" class="release">
          <header class="release-head">
            <span class="release-version mono">v{{ r.version }}</span>
            <span class="muted release-date">{{ r.date }}</span>
          </header>
          <p class="release-title">{{ r.title }}</p>
          <div v-for="g in r.groups" :key="g.kind" class="change-group">
            <span class="change-kind" :class="`kind-${g.kind}`">{{ g.label }}</span>
            <ul class="change-list">
              <li v-for="item in g.items" :key="item">{{ item }}</li>
            </ul>
          </div>
        </article>
      </details>
    </section>

    <section class="card-surface about-card">
      <div class="section-title">主要功能</div>
      <ul class="about-list">
        <li v-for="f in features" :key="f">{{ f }}</li>
      </ul>
    </section>

    <section class="card-surface about-card">
      <div class="section-title">后端</div>
      <div v-for="[name, note] in backendStack" :key="name" class="stack-row">
        <code class="mono stack-name">{{ name }}</code>
        <span class="muted stack-note">{{ note }}</span>
      </div>
    </section>

    <section class="card-surface about-card">
      <div class="section-title">前端</div>
      <div v-for="[name, note] in frontendStack" :key="name" class="stack-row">
        <code class="mono stack-name">{{ name }}</code>
        <span class="muted stack-note">{{ note }}</span>
      </div>
    </section>

    <section class="card-surface about-card">
      <div class="section-title">部署</div>
      <div v-for="[name, note] in deployStack" :key="name" class="stack-row">
        <code class="mono stack-name">{{ name }}</code>
        <span class="muted stack-note">{{ note }}</span>
      </div>
    </section>

    <section v-if="links.length" class="card-surface about-card">
      <div class="section-title">仓库与反馈</div>
      <div v-for="l in links" :key="l.label" class="stack-row">
        <span class="stack-name">{{ l.label }}</span>
        <a
          class="stack-note"
          :href="l.href"
          :target="l.external ? '_blank' : undefined"
          :rel="l.external ? 'noopener' : undefined"
          >{{ l.text }}</a
        >
      </div>
    </section>
  </div>
</template>

<style scoped>
.about-page {
  max-width: 880px;
}
.about-hero {
  padding: 22px 24px;
  margin-bottom: 16px;
}
.about-title {
  margin: 0;
  font-size: 26px;
  letter-spacing: 0.5px;
}
.about-sub {
  margin: 6px 0 0;
  font-size: 14px;
}
.about-meta {
  margin-top: 14px;
  gap: 8px;
  font-size: 13px;
}
.about-meta .dot {
  color: var(--rule);
}
.about-card {
  padding: 16px 18px;
  margin-bottom: 16px;
}
.about-card-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}
.about-card-note {
  font-size: 12px;
}
.release {
  padding: 10px 0;
  border-top: 1px solid var(--rule-soft);
}
.release:first-of-type {
  padding-top: 0;
  border-top: 0;
}
.release-head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.release-version {
  font-size: 15px;
  font-weight: 600;
}
.release-badge {
  padding: 0 7px;
  border: 1px solid var(--rule-soft);
  border-radius: 999px;
  background: var(--brand-soft);
  color: var(--brand);
  font-size: 11px;
  line-height: 18px;
}
.release-date {
  margin-left: auto;
  font-size: 12px;
}
.release-title {
  margin: 5px 0 8px;
  color: var(--ink-2);
  font-size: 13px;
}
.change-group {
  display: flex;
  gap: 10px;
  padding: 4px 0;
}
.change-kind {
  flex: none;
  width: 46px;
  height: 20px;
  border: 1px solid var(--rule-soft);
  border-radius: 4px;
  background: var(--bg-soft);
  color: var(--muted);
  font-size: 11.5px;
  line-height: 18px;
  text-align: center;
}
.kind-added {
  color: #1a7f37;
  border-color: color-mix(in srgb, #1a7f37 35%, transparent);
}
.kind-improved {
  color: var(--brand);
  border-color: color-mix(in srgb, var(--brand) 35%, transparent);
}
.kind-fixed {
  color: #bc4c00;
  border-color: color-mix(in srgb, #bc4c00 35%, transparent);
}
.kind-security {
  color: #cf222e;
  border-color: color-mix(in srgb, #cf222e 35%, transparent);
}
:global(html.dark) .kind-added {
  color: #3fb950;
}
:global(html.dark) .kind-fixed {
  color: #f0883e;
}
:global(html.dark) .kind-security {
  color: #ff7b72;
}
.change-list {
  margin: 0;
  padding-left: 16px;
  font-size: 13px;
  line-height: 1.8;
}
.change-list li {
  margin: 0;
}
.release-history {
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px solid var(--rule-soft);
}
.release-history summary {
  cursor: pointer;
  color: var(--muted);
  font-size: 12.5px;
}
.about-list {
  margin: 0;
  padding-left: 20px;
  font-size: 13.5px;
  line-height: 2;
}
.stack-row {
  display: flex;
  gap: 12px;
  padding: 6px 0;
  border-top: 1px solid var(--rule-soft);
  font-size: 13px;
}
.stack-row:first-of-type {
  border-top: 0;
}
.stack-name {
  flex: none;
  width: 250px;
  word-break: break-word;
}
.stack-note {
  min-width: 0;
  overflow-wrap: anywhere;
}
@media (max-width: 640px) {
  .stack-row {
    flex-direction: column;
    gap: 2px;
  }
  .stack-name {
    width: auto;
  }
}
</style>
