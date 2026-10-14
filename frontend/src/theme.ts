import type { GlobalThemeOverrides } from 'naive-ui'

/**
 * Naive UI theme overrides derived from the design tokens in styles/global.css,
 * so components follow the same GitHub-flavoured palette as the hand-written
 * markup (light and dark).
 */
const FONT_SANS =
  "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', 'Helvetica Neue', Arial, sans-serif"
const FONT_MONO =
  "ui-monospace, 'JetBrains Mono', SFMono-Regular, Consolas, 'Liberation Mono', monospace"

const shared: GlobalThemeOverrides = {
  common: {
    fontFamily: FONT_SANS,
    fontFamilyMono: FONT_MONO,
    fontSize: '15px',
    borderRadius: '8px',
    borderRadiusSmall: '6px',
  },
}

export const lightThemeOverrides: GlobalThemeOverrides = {
  common: {
    ...shared.common,
    primaryColor: '#0969da',
    primaryColorHover: '#0550ae',
    primaryColorPressed: '#0550ae',
    primaryColorSuppl: '#0550ae',
    bodyColor: '#ffffff',
    cardColor: '#ffffff',
    modalColor: '#ffffff',
    popoverColor: '#ffffff',
    tableColor: '#ffffff',
    inputColor: '#ffffff',
    textColorBase: '#1f2328',
    textColor1: '#1f2328',
    textColor2: '#424a53',
    textColor3: '#656d76',
    borderColor: '#d0d7de',
    dividerColor: '#d0d7de',
    hoverColor: '#f6f8fa',
    tableHeaderColor: '#f6f8fa',
    scrollbarColor: '#d0d7de',
    scrollbarColorHover: '#656d76',
  },
  Tree: {
    nodeColorHover: '#eaeef2',
    nodeColorActive: '#e7f0fb',
    nodeTextColor: '#424a53',
    nodeTextColorActive: '#0550ae',
    nodeBorderRadius: '6px',
    fontSize: '13px',
  },
}

export const darkThemeOverrides: GlobalThemeOverrides = {
  common: {
    ...shared.common,
    primaryColor: '#4493f8',
    primaryColorHover: '#6cb6ff',
    primaryColorPressed: '#6cb6ff',
    primaryColorSuppl: '#4493f8',
    bodyColor: '#0d1117',
    cardColor: '#161b22',
    modalColor: '#161b22',
    popoverColor: '#161b22',
    tableColor: '#0d1117',
    inputColor: '#0d1117',
    textColorBase: '#e6edf3',
    textColor1: '#e6edf3',
    textColor2: '#c9d1d9',
    textColor3: '#8b949e',
    borderColor: '#30363d',
    dividerColor: '#30363d',
    hoverColor: '#21262d',
    tableHeaderColor: '#21262d',
    scrollbarColor: '#30363d',
    scrollbarColorHover: '#8b949e',
  },
  Tree: {
    nodeColorHover: '#21262d',
    nodeColorActive: '#12243d',
    nodeTextColor: '#c9d1d9',
    nodeTextColorActive: '#4493f8',
    nodeBorderRadius: '6px',
    fontSize: '13px',
  },
}
