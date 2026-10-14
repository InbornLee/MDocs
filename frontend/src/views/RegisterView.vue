<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { NForm, NFormItem, NInput, NButton, useMessage } from 'naive-ui'
import { api } from '@/api/client'
import { useAuthStore, type UserDto } from '@/stores/auth'

const router = useRouter()
const message = useMessage()
const auth = useAuthStore()

const username = ref('')
const email = ref('')
const password = ref('')
const confirmPassword = ref('')
const loading = ref(false)

async function submit() {
  if (username.value.length < 3) {
    message.warning('用户名至少 3 个字符')
    return
  }
  if (password.value.length < 8) {
    message.warning('密码至少 8 个字符')
    return
  }
  if (password.value !== confirmPassword.value) {
    message.warning('两次输入的密码不一致')
    return
  }
  loading.value = true
  try {
    const data = await api.post<{ token: string; user: UserDto }>(
      '/api/auth/register',
      { username: username.value, email: email.value, password: password.value },
    )
    auth.setSession(data.token, data.user)
    message.success('注册成功，已自动登录')
    router.push('/projects')
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
      <h2 class="auth-title">创建账户</h2>
      <n-form @keyup.enter="submit">
        <n-form-item label="用户名">
          <n-input v-model:value="username" placeholder="3-64 位字母、数字、下划线或连字符" />
        </n-form-item>
        <n-form-item label="邮箱">
          <n-input v-model:value="email" placeholder="you@example.com" />
        </n-form-item>
        <n-form-item label="密码">
          <n-input v-model:value="password" type="password" show-password-on="click" placeholder="至少 8 个字符" />
        </n-form-item>
        <n-form-item label="确认密码">
          <n-input
            v-model:value="confirmPassword"
            type="password"
            show-password-on="click"
            placeholder="再输入一次密码"
          />
        </n-form-item>
        <n-button type="primary" block :loading="loading" @click="submit">注册</n-button>
        <p class="auth-foot muted">
          已有账号？<a @click="router.push('/login')">登录</a>
        </p>
      </n-form>
    </section>
  </div>
</template>

<style scoped>
.auth-wrap {
  display: flex;
  justify-content: center;
  padding-top: 60px;
}
.auth-card {
  width: 420px;
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
