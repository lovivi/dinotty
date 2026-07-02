<template>
  <div class="settings-section">
    <h3 class="section-title">
      <span>{{ t('remote.title') }}</span>
      <button class="help-btn" @click="showHelp = true" title="Setup guide">?</button>
    </h3>

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

    <!-- Help / setup guide dialog -->
    <div v-if="showHelp" class="modal-backdrop" @click.self="showHelp = false">
      <div class="help-modal">
        <div class="help-modal-header">
          <h2>{{ t('remote.helpTitle') }}</h2>
          <button class="modal-close" @click="showHelp = false">×</button>
        </div>

        <div class="help-modal-body">
          <h3>1. {{ t('remote.helpStep1Title') }}</h3>
          <p class="help-step-desc">{{ t('remote.helpStep1Desc') }}</p>
          <div class="help-code-block">
            <code>curl -sSL https://raw.githubusercontent.com/lovivi/dinotty/feat/windows-shell-profiles/deploy/relay/server-install.sh | sudo bash -s -- mypassword123</code>
            <button class="help-copy-btn" @click="copyText('curl -sSL https://raw.githubusercontent.com/lovivi/dinotty/feat/windows-shell-profiles/deploy/relay/server-install.sh | sudo bash -s -- mypassword123')">
              {{ copied === 'server' ? '✓' : 'Copy' }}
            </button>
          </div>
          <p class="help-step-hint">{{ t('remote.helpStep1Hint') }}</p>

          <h3>2. {{ t('remote.helpStep2Title') }}</h3>
          <p class="help-step-desc">{{ t('remote.helpStep2Desc') }}</p>
          <div class="help-code-block">
            <code>E:\dinotty-builds\Dinotty_0.12.1_mobile_v3.apk</code>
            <button class="help-copy-btn" @click="copyText('E:\\dinotty-builds\\Dinotty_0.12.1_mobile_v3.apk')">
              {{ copied === 'apk' ? '✓' : 'Copy' }}
            </button>
          </div>

          <h3>3. {{ t('remote.helpStep3Title') }}</h3>
          <p class="help-step-desc">{{ t('remote.helpStep3Desc') }}</p>

          <div class="help-quick-ref">
            <h4>{{ t('remote.helpQuickRef') }}</h4>
            <table>
              <tr><th>{{ t('remote.helpColItem') }}</th><th>{{ t('remote.helpColWhere') }}</th></tr>
              <tr><td>Server VPS one-liner</td><td>{{ t('remote.helpRefServer') }}</td></tr>
              <tr><td>APK for phone</td><td>{{ t('remote.helpRefApk') }}</td></tr>
              <tr><td>Windows desktop</td><td>{{ t('remote.helpRefWindows') }}</td></tr>
            </table>
          </div>
        </div>
      </div>
    </div>
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
const showHelp = ref(false)
const copied = ref<string | null>(null)

const RELAY_STORAGE_KEY = 'dinotty_relay_config'

async function copyText(text: string) {
  const key = text.startsWith('curl') ? 'server' : 'apk'
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text)
    } else {
      const ta = document.createElement('textarea')
      ta.value = text
      ta.style.position = 'fixed'
      ta.style.opacity = '0'
      document.body.appendChild(ta)
      ta.select()
      document.execCommand('copy')
      document.body.removeChild(ta)
    }
    copied.value = key
    setTimeout(() => { if (copied.value === key) copied.value = null }, 1500)
  } catch (e) {
    console.error('copy failed', e)
  }
}

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

/* ── Section title with help button ─────────────────────────── */
.section-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.help-btn {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  border: 1px solid var(--border, #333);
  background: var(--bg-input, #1a1a1a);
  color: var(--fg-muted, #666);
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
}
.help-btn:hover {
  border-color: var(--accent, #7C3AED);
  color: var(--accent, #7C3AED);
}

/* ── Help modal ───────────────────────────────────────────── */
.modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
}
.help-modal {
  background: var(--bg-surface, #1a1a1a);
  border: 1px solid var(--border, #333);
  border-radius: 8px;
  max-width: 560px;
  width: 100%;
  max-height: 86vh;
  overflow-y: auto;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
}
.help-modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  border-bottom: 1px solid var(--border, #333);
}
.help-modal-header h2 {
  font-size: 14px;
  font-weight: 600;
  color: var(--fg-bright, #f0f6fc);
}
.modal-close {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  background: none;
  border: none;
  color: var(--fg-muted, #666);
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  padding: 0;
}
.modal-close:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--fg-bright, #f0f6fc);
}
.help-modal-body {
  padding: 16px 18px 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.help-modal-body h3 {
  font-size: 13px;
  font-weight: 600;
  color: var(--fg-bright, #f0f6fc);
  margin-top: 6px;
}
.help-step-desc,
.help-step-hint {
  font-size: 12px;
  color: var(--fg-muted, #888);
  line-height: 1.5;
}
.help-step-hint {
  margin-top: -4px;
}
.help-code-block {
  display: flex;
  align-items: center;
  gap: 6px;
  background: var(--bg-input, #0a0a0a);
  border: 1px solid var(--border, #333);
  border-radius: 4px;
  padding: 6px 8px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--fg, #c7c7c7);
  word-break: break-all;
}
.help-code-block code {
  flex: 1;
  background: none;
  border: none;
  padding: 0;
  color: inherit;
  font-family: inherit;
  font-size: inherit;
}
.help-copy-btn {
  flex-shrink: 0;
  padding: 2px 8px;
  border-radius: 3px;
  font-size: 10px;
  background: var(--accent, #7C3AED);
  color: #fff;
  border: none;
  cursor: pointer;
}
.help-copy-btn:hover {
  opacity: 0.85;
}
.help-quick-ref {
  margin-top: 4px;
  background: var(--bg-input, #0a0a0a);
  border: 1px solid var(--border, #333);
  border-radius: 4px;
  padding: 10px 12px;
}
.help-quick-ref h4 {
  font-size: 11px;
  font-weight: 600;
  color: var(--fg-muted, #666);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin-bottom: 6px;
}
.help-quick-ref table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}
.help-quick-ref th,
.help-quick-ref td {
  text-align: left;
  padding: 3px 6px;
  color: var(--fg, #c7c7c7);
}
.help-quick-ref th {
  color: var(--fg-muted, #666);
  font-weight: 500;
}
</style>
