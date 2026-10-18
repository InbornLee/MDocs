/**
 * 版本更新日志。
 *
 * 单一数据源：关于页按这里的顺序渲染（第一条即当前版本）。
 * 发版时同时更新 `frontend/package.json` 与本文件。
 */

export type ChangeKind = 'added' | 'improved' | 'fixed' | 'security'

export interface ChangeGroup {
  kind: ChangeKind
  label: string
  items: string[]
}

export interface Release {
  version: string
  /** ISO 日期，页面直接展示为 YYYY-MM-DD */
  date: string
  title: string
  groups: ChangeGroup[]
}

export const changelog: Release[] = [
  {
    version: '0.1.1',
    date: '2026-10-18',
    title: 'AI 知识库 P0：可溯源检索、分段读取与调用治理',
    groups: [
      {
        kind: 'added',
        label: '新增',
        items: [
          'MCP 工具 get_document_outline 与 list_branches，工具总数达到 7 个',
          'MCP Resources：mdocs://{project}/{path} 与资源模板，客户端可按 URI 直接拉取文档',
          'MCP Prompt answer_with_citations：引导模型先检索、按节精读，再附引用作答',
          'GET /api/version：返回版本号、工具清单与限流配置',
          'API token 有效期（7 / 30 / 90 天或永久），过期后返回明确错误',
          'MCP 调用审计表 mcp_audit_log：记录工具、项目、调用者、IP 与结果',
        ],
      },
      {
        kind: 'improved',
        label: '改进',
        items: [
          '检索改为相关度排序（标题 > 路径 > 正文），返回 score、anchor、url 与 commit_id',
          'read_document 支持 section / offset / limit，返回 total_bytes 与 truncated',
          'search_documents 支持 offset 与跨项目检索（projects）',
          '分支快照缓存：同一 head commit 不再重复扫描全部历史',
          'MCP 每分钟限流（默认 240 次，可按 IP / token 配置）',
          '部署脚本同时生成版本 tag，镜像可一键回滚',
        ],
      },
      {
        kind: 'fixed',
        label: '修复',
        items: [
          'YAML front matter 被误识别为标题，导致大纲锚点与阅读页不一致',
          '构建上下文包含 deploy/.env 的风险（.dockerignore 已排除）',
        ],
      },
    ],
  },
  {
    version: '0.1.0',
    date: '2026-10-14',
    title: '首个版本：类 Git 的 Markdown 文档管理与基础 MCP',
    groups: [
      {
        kind: 'added',
        label: '首个版本',
        items: [
          '文档树、在线编辑、附件与全文检索',
          '分支、commit 历史、版本回溯、行级三方合并与合并请求',
          '自动链接树：相对链接与 [[wiki]] 链接、悬空链接检测',
          '导入导出：zip / 单文件 / tree JSON，兼容 GBK 文件名',
          '公开站点与按文档发布',
          'MCP 基础工具：list_projects / list_documents / read_document / search_documents / get_link_graph',
          'SQLite 默认、PostgreSQL 可选，单容器部署',
        ],
      },
    ],
  },
]
