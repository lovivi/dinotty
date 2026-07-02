<template>
  <div class="settings-section">
    <h3>{{ t('remote.title') }}</h3>

    <div class="remote-status-row">
      <span class="remote-status-dot" :class="statusClass" />
      <span class="remote-status-text">{{ statusText }}</span>
    </div>

    <div class="settings-row">
      <label>{{ t('remote.relayUrl') }}</label>
      <input v-model="relayUrl" type="text" class="settings-input" placeholder="wss://12.34.56.78:24020" />
    </div>
    <div class="settings-row">
      <label>{{ t('remote.password') }}</label>
      <input v-model="password" type="password" class="settings-input" placeholder="********" />
    </div>

    <div v-if="desktopId" class="remote-id-row">
      <label>{{ t('remote.desktopId') }}</label>
      <code class="remote-id-value">{{ desktopId }}</code>
    </div>

    <button
      class="remote-connect-btn"
      :disabled="!relayUrl.trim() || !password.trim() || connecting"
      @click="toggleConnect"
    >
      {{ connecting ? t('remote.connecting') : connected ? t('remote.disconnect') : t('remote.connect') }}
    </button>

    <p v-if="!isTauri" class="remote-hint">
      {{ t('remote.desktopOnlyHint') }}
    </p>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from '../../composables/useI18n'
import { isTauri, tauriInvoke } from '../../composables/useTransport'
import { getDesktopId, setDesktopId } from '../../composables/apiBase'

const { t } = useI18n()

const relayUrl = ref('')
const password = ref('')
const desktopId = ref('')
const connected = ref(false)
const connecting = ref(false)

const RELAY_STORAGE_KEY = 'dinotty_relay_config'

const statusClass = computed(() => {
  if (connecting.value) return 'status-connecting'
  if (connected.value) return 'status-connected'
  return 'status-disconnected'
})

const statusText = computed(() => {
  if (connecting.value) return t('remote.statusConnecting')
  if (connected.value) return t('remote.statusConnected')
  return t('remote.statusDisconnected')
})

onMounted(() => {
  try {
    const raw = localStorage.getItem(RELAY_STORAGE_KEY)
    if (raw) {
      const cfg = JSON.parse(raw)
      if (cfg.url) relayUrl.value = cfg.url
      if (cfg.password) password.value = cfg.password
    }
  } catch { /* ignore */ }
  desktopId.value = getDesktopId() || ''
})

async function toggleConnect() {
  if (connected.value) {
    await disconnect()
    return
  }
  await connect()
}

async function connect() {
  if (!isTauri) return
  connecting.value = true
  try {
    await tauriInvoke('relay_connect', {
      url: relayUrl.value.trim(),
      password: password.value.trim(),
    })
    connected.value = true
    desktopId.value = getDesktopId() || ''
    // Save config
    localStorage.setItem(RELAY_STORAGE_KEY, JSON.stringify({
      url: relayUrl.value.trim(),
      password: password.value.trim(),
    }))
  } catch (e) {
    console.error('relay connect failed:', e)
  } finally {
    connecting.value = false
  }
}

async function disconnect() {
  if (!isTauri) return
  try {
    await tauriInvoke('relay_disconnect')
  } catch (e) {
    console.error('relay disconnect failed:', e)
  }
  connected.value = false
}
</script>

<style scoped>
.remote-status-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 16px;
}
.remote-status-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}
.status-connected { background: #27ae60; box-shadow: 0 0 6px #27ae60; }
.status-disconnected { background: #c0392b; }
.status-connecting { background: #f39c12; animation: pulse 1s infinite; }
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}
.remote-status-text {
  font-size: 13px;
  color: var(--fg, #c7c7c7);
}
.remote-id-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
  font-size: 12px;
}
.remote-id-row label {
  color: var(--fg-muted, #666);
}
.remote-id-value {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--fg-muted, #888);
  background: var(--bg-input, #1a1a1a);
  padding: 2px 6px;
  border-radius: 3px;
}
.remote-connect-btn {
  width: 100%;
  padding: 10px;
  border-radius: 6px;
  font-size: 14px;
  font-weight: 500;
  background: var(--accent, #7C3AED);
  color: #fff;
  border: none;
  margin-bottom: 10px;
  transition: opacity 0.15s;
}
.remote-connect-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.remote-connect-btn:hover:not(:disabled) {
  opacity: 0.85;
}
.remote-hint {
  font-size: 11px;
  color: var(--fg-muted, #666);
  text-align: center;
}

.settings-input {
  flex: 1;
  background: var(--bg-input, #1a1a1a);
  border: 1px solid var(--border, #333);
  border-radius: 4px;
  color: var(--fg, #c7c7c7);
  padding: 6px 8px;
  font-size: 13px;
  font-family: var(--font-mono);
  width: 0;
  min-width: 0;
}
.settings-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 10px;
}
.settings-row label {
  font-size: 13px;
  color: var(--fg, #c7c7c7);
  white-space: nowrap;
  min-width: 80px;
}
</style>
