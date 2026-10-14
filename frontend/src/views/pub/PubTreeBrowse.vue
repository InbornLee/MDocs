<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NTree, type TreeOption } from 'naive-ui'
import { usePublicProjectStore, type TreeNode } from '@/stores/publicProject'
import { encodeDocPath } from '@/utils/markdown'
import { renderTreeLabel } from '@/utils/treeLabel'

const route = useRoute()
const router = useRouter()
const store = usePublicProjectStore()

const slug = computed(() => route.params.slug as string)
const treeData = computed(() => store.tree as unknown as TreeOption[])

const defaultExpanded = computed(() =>
  // 父节点可能是文档本身，所以按「有没有子节点」判断
  store.tree.filter((n) => n.children?.length).map((n) => n.key),
)
const expandedKeys = ref<string[]>(defaultExpanded.value)
const selectedKeys = ref<string[]>([])

function nodeProps({ option }: { option: TreeOption }) {
  const node = option as unknown as TreeNode
  return {
    onClick(e: MouseEvent) {
      // 点展开箭头时交给 n-tree 自己处理，避免和这里的折叠逻辑互相抵消
      if ((e.target as HTMLElement | null)?.closest('.n-tree-node-switcher')) return
      if (node.isDoc && node.doc) {
        router.push(`/pub/${slug.value}/view/${encodeDocPath(node.doc.path)}`)
      } else if (!node.isDoc) {
        expandedKeys.value = expandedKeys.value.includes(node.key)
          ? expandedKeys.value.filter((k) => k !== node.key)
          : [...expandedKeys.value, node.key]
      }
    },
  }
}
</script>

<template>
  <div class="page">
    <div class="row" style="margin-bottom: 12px; font-weight: 600; font-size: 15px">
      <span>文档目录</span>
      <span class="muted" style="font-weight: 400; font-size: 13px">
        {{ store.docs.length }} 篇
      </span>
    </div>

    <p v-if="!store.docs.length" class="empty-state">该项目尚未发布任何文档</p>
    <n-tree
      v-else
      v-model:expanded-keys="expandedKeys"
      v-model:selected-keys="selectedKeys"
      :data="treeData"
      :node-props="nodeProps"
      :render-label="renderTreeLabel"
      block-line
      selectable
      :default-expanded-keys="defaultExpanded"
    />
  </div>
</template>
