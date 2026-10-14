import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { BranchDto, DocListItem, ProjectDto } from '@/api/types'
import { getProject, listDocs } from '@/api/projects'
import { buildDocTree, type DocTreeNode } from '@/utils/docTree'

export type TreeNode = DocTreeNode<DocListItem>

export const useProjectStore = defineStore('project', () => {
  const project = ref<ProjectDto | null>(null)
  const branches = ref<BranchDto[]>([])
  const commitCount = ref(0)
  const branchName = ref<string>('')
  const docs = ref<DocListItem[]>([])
  const docsLoading = ref(false)

  const branch = computed(() => branches.value.find((b) => b.name === branchName.value) || null)
  const tree = computed(() => buildDocTree(docs.value))
  const canWrite = computed(() =>
    project.value ? project.value.role === 'owner' || project.value.role === 'editor' : false,
  )

  function docByPath(path: string): DocListItem | undefined {
    return docs.value.find((d) => d.path === path)
  }

  async function loadProject(slug: string, opts?: { keepBranch?: boolean }) {
    const data = await getProject(slug)
    project.value = data.project
    branches.value = data.branches
    commitCount.value = data.commit_count
    if (!opts?.keepBranch) {
      branchName.value = data.project.default_branch || data.branches[0]?.name || 'main'
    }
  }

  async function loadDocs(branch?: string) {
    if (!project.value) return
    docsLoading.value = true
    try {
      const b = branch ?? branchName.value
      const data = await listDocs(project.value.slug, b)
      branchName.value = data.branch.name
      docs.value = data.docs
    } finally {
      docsLoading.value = false
    }
  }

  function $reset() {
    project.value = null
    branches.value = []
    commitCount.value = 0
    branchName.value = ''
    docs.value = []
  }

  return {
    project,
    branches,
    commitCount,
    branchName,
    branch,
    docs,
    docsLoading,
    tree,
    canWrite,
    docByPath,
    loadProject,
    loadDocs,
    $reset,
  }
})
