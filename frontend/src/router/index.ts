import { createRouter, createWebHistory } from 'vue-router'
import { getToken } from '@/api/client'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'home',
      component: () => import('@/views/HomeView.vue'),
    },
    {
      path: '/about',
      name: 'about',
      component: () => import('@/views/AboutView.vue'),
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
    },
    {
      path: '/register',
      name: 'register',
      component: () => import('@/views/RegisterView.vue'),
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/views/SettingsView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/projects',
      name: 'projects',
      component: () => import('@/views/ProjectsView.vue'),
      meta: { requiresAuth: true },
    },
    {
      // 管理员专属；页面内部再校验一次身份
      path: '/admin',
      name: 'admin',
      component: () => import('@/views/AdminView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/p/:slug',
      component: () => import('@/views/project/ProjectLayout.vue'),
      meta: { requiresAuth: true },
      children: [
        { path: '', redirect: (to) => `/p/${to.params.slug}/tree` },
        { path: 'tree', name: 'project-tree', component: () => import('@/views/project/TreeBrowse.vue') },
        // Vue Router 4 needs an explicit repeatable catch-all param; the
        // Vue Router 3 form `view/*` silently matches nothing.
        { path: 'view/:pathMatch(.*)*', name: 'doc-view', component: () => import('@/views/project/DocView.vue') },
        { path: 'edit/:pathMatch(.*)*', name: 'doc-edit', component: () => import('@/views/project/DocEdit.vue') },
        { path: 'history/:pathMatch(.*)*', name: 'doc-history', component: () => import('@/views/project/DocHistory.vue') },
        { path: 'links', name: 'project-links', component: () => import('@/views/project/LinksView.vue') },
        { path: 'branches', name: 'project-branches', component: () => import('@/views/project/BranchesView.vue') },
        { path: 'mrs', name: 'project-mrs', component: () => import('@/views/project/MrListView.vue') },
        { path: 'mrs/:mrId', name: 'mr-detail', component: () => import('@/views/project/MrDetailView.vue') },
        { path: 'members', name: 'project-members', component: () => import('@/views/project/MembersView.vue') },
        { path: 'import-export', name: 'project-import-export', component: () => import('@/views/project/ImportExportView.vue') },
      ],
    },
    {
      path: '/pub/:slug',
      component: () => import('@/views/pub/PubLayout.vue'),
      children: [
        { path: '', redirect: (to) => `/pub/${to.params.slug}/tree` },
        { path: 'tree', name: 'pub-tree', component: () => import('@/views/pub/PubTreeBrowse.vue') },
        { path: 'view/:pathMatch(.*)*', name: 'pub-doc-view', component: () => import('@/views/pub/PubDocView.vue') },
        { path: 'links', name: 'pub-links', component: () => import('@/views/pub/PubLinksView.vue') },
      ],
    },
  ],
})

router.beforeEach((to) => {
  if (to.meta.requiresAuth && !getToken()) {
    return { name: 'login', query: { redirect: to.fullPath } }
  }
})

export default router
