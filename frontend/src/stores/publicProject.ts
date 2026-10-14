import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { getPublicProject, type PublicDocItem, type PublicProjectDetail } from '@/api/public'
import { buildDocTree, type DocTreeNode } from '@/utils/docTree'

export type TreeNode = DocTreeNode<PublicDocItem>

export const usePublicProjectStore = defineStore('publicProject', () => {
  const detail = ref<PublicProjectDetail | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  const tree = computed(() => buildDocTree(detail.value?.docs ?? []))
  const docs = computed(() => detail.value?.docs ?? [])

  function docByPath(path: string) {
    return docs.value.find((d) => d.path === path)
  }

  async function load(slug: string, branch?: string) {
    loading.value = true
    error.value = null
    try {
      detail.value = await getPublicProject(slug, branch)
    } catch (e: unknown) {
      detail.value = null
      error.value = (e as Error).message || '加载失败'
    } finally {
      loading.value = false
    }
  }

  function $reset() {
    detail.value = null
    error.value = null
    loading.value = false
  }

  return { detail, loading, error, tree, docs, docByPath, load, $reset }
})
