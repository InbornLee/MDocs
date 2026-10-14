<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NForm, NFormItem, NInput, NButton, useMessage } from 'naive-ui'
import { api } from '@/api/client'
import { useAuthStore, type UserDto } from '@/stores/auth'

const router = useRouter()
const route = useRoute()
const message = useMessage()
const auth = useAuthStore()

const username = ref('')
const password = ref('')
const loading = ref(false)

async function submit() {
  if (!username.value || !password.value) {
    message.warning('请填写用户名和密码')
    return
  }
  loading.value = true
  try {
    const data = await api.post<{ token: string; user: UserDto }>(
      '/api/auth/login',
      { username: username.value, password: password.value },
    )
    auth.setSession(data.token, data.user)
    message.success('登录成功')
    router.push((route.query.redirect as string) || '/projects')
  } catch (e: unknown) {
    message.error(String((e as Error)?.message ?? e))
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="page auth-wrap">
    <section class="card-surface auth-card">
      <h2 class="auth-title">登录</h2>
      <n-form @keyup.enter="submit">
        <n-form-item label="用户名或邮箱">
          <n-input v-model:value="username" placeholder="username / email" />
        </n-form-item>
        <n-form-item label="密码">
          <n-input v-model:value="password" type="password" show-password-on="click" placeholder="password" />
        </n-form-item>
        <n-button type="primary" block :loading="loading" @click="submit">登录</n-button>
        <p class="auth-foot muted">
          还没有账号？<a @click="router.push('/register')">注册</a>
        </p>
      </n-form>
    </section>
  </div>
</template>

<style scoped>
.auth-wrap {
  display: flex;
  justify-content: center;
  padding-top: 80px;
}
.auth-card {
  width: 400px;
  padding: 20px 22px 24px;
  box-shadow: var(--shadow-sm);
}
.auth-title {
  margin: 0 0 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--rule-soft);
  font-size: 17px;
  font-weight: 700;
}
.auth-foot {
  margin: 12px 0 0;
  text-align: center;
  font-size: 13px;
}
.auth-foot a {
  cursor: pointer;
}
</style>
