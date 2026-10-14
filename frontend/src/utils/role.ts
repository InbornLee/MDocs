/** 项目角色在界面上的中文名（与成员管理里的叫法保持一致） */
export const ROLE_LABELS: Record<string, string> = {
  owner: '拥有者',
  editor: '编辑者',
  reader: '只读',
}

/** 角色 → 中文名；未知角色原样返回 */
export function roleLabel(role: string): string {
  return ROLE_LABELS[role] ?? role
}
