<template>
  <div class="remote-screen">
    <header class="remote-screen-header">
      <div class="remote-screen-meta">
        <span class="dot" :class="{ connected }" />
        <span>{{ connected ? 'connected' : 'connecting…' }}</span>
        <span class="sep">·</span>
        <span class="pane-id">{{ paneId ?? '—' }}</span>
        <span class="sep">·</span>
        <span class="cols-rows">{{ cols }}×{{ rows }}</span>
        <span class="sep">·</span>
        <span class="fps">{{ fps }} fps</span>
      </div>
      <div class="remote-screen-tag">read-only · v0</div>
    </header>
    <pre ref="termEl" class="remote-screen-term">{{ screen }}</pre>
    <form class="remote-screen-input" @submit.prevent="sendInput">
      <input
        ref="inputEl"
        v-model="inputText"
        class="input-field"
        type="text"
        placeholder="Type a command…"
        autocomplete="off"
        autocorrect="off"
        spellcheck="false"
        @keyup.esc="inputText = ''; (inputEl as any)?.blur()"
      />
      <button type="submit" class="input-btn" :disabled="!inputText.trim()">
        Send
      </button>
    </form>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, nextTick } from 'vue'

const props = defineProps<{
  relayUrl: string
  desktopId: string
  token: string
}>()

const connected = ref(false)
const paneId = ref<string | null>(null)
const cols = ref(0)
const rows = ref(0)
const screen = ref('')
const termEl = ref<HTMLPreElement | null>(null)
const inputEl = ref<HTMLInputElement | null>(null)
const inputText = ref('')

// FPS counter
let frameCount = 0
let lastFpsTick = performance.now()
const fps = ref(0)

let ws: WebSocket | null = null
let reconnectAttempts = 0
let reconnectTimer: number | null = null
let alive = true

function scheduleReconnect() {
  if (!alive) return
  if (reconnectTimer !== null) return
  const delay = Math.min(1000 * 2 ** reconnectAttempts, 30000)
  reconnectAttempts += 1
  reconnectTimer = window.setTimeout(() => {
    reconnectTimer = null
    connect()
  }, delay)
}

function connect() {
  // Build a wss:// or ws:// URL from the relay URL we were given.
  // The relay's mobile-ws endpoint is /relay/mobile/ws/<desktop_id>;
  // auth via Authorization: Bearer or ?token=.
  const base = props.relayUrl
    .replace(/^http/, 'ws')
    .replace(/\/$/, '')
  const url = `${base}/relay/mobile/ws/${encodeURIComponent(
    props.desktopId
  )}?token=${encodeURIComponent(props.token)}`

  ws = new WebSocket(url)
  ws.onopen = () => {
    connected.value = true
    reconnectAttempts = 0
  }
  ws.onclose = () => {
    connected.value = false
    scheduleReconnect()
  }
  ws.onerror = () => {
    // onclose will fire too
  }
  ws.onmessage = (e) => {
    try {
      const msg = JSON.parse(e.data) as {
        type: string
        pane_id?: string
        screen?: string
        cols?: number
        rows?: number      }
      if (msg.type === 'screen' && typeof msg.screen === 'string') {
        paneId.value = msg.pane_id ?? null
        if (msg.cols) cols.value = msg.cols
        if (msg.rows) rows.value = msg.rows
        screen.value = msg.screen
        if (termEl.value) termEl.value.scrollTop = termEl.value.scrollHeight

        frameCount += 1
        const now = performance.now()
        if (now - lastFpsTick > 1000) {
          fps.value = Math.round((frameCount * 1000) / (now - lastFpsTick))
          frameCount = 0
          lastFpsTick = now
        }
      }
    } catch {
      // ignore malformed frames
    }
  }
}

onMounted(() => {
  connect()
})

function sendInput() {
  const text = inputText.value.trim()
  if (!text || !ws || ws.readyState !== WebSocket.OPEN) return
  ws.send(JSON.stringify({ type: 'input', data: text + '\r' }))
  inputText.value = ''
  // Focus back on the input after sending
  nextTick(() => inputEl.value?.focus())
}

onBeforeUnmount(() => {
  alive = false
  if (reconnectTimer !== null) {
    window.clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
  if (ws) {
    ws.onclose = null
    ws.onerror = null
    ws.onmessage = null
    ws.onopen = null
    ws.close()
    ws = null
  }
})
</script>

<style scoped>
.remote-screen {
  position: fixed;
  inset: 0;
  background: #000;
  color: #ccc;
  display: flex;
  flex-direction: column;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  z-index: 9999;
}

.remote-screen-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 14px;
  background: #1a1a1a;
  border-bottom: 1px solid #2a2a2a;
  font-size: 11px;
  color: #888;
  user-select: none;
  -webkit-user-select: none;
}

.remote-screen-meta {
  display: flex;
  align-items: center;
  gap: 6px;
}

.remote-screen-meta .dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #c0392b;
  margin-right: 4px;
}

.remote-screen-meta .dot.connected {
  background: #27ae60;
  box-shadow: 0 0 6px #27ae60;
}

.remote-screen-meta .sep {
  color: #444;
}

.remote-screen-tag {
  font-size: 10px;
  color: #555;
  letter-spacing: 0.05em;
}

.remote-screen-term {
  flex: 1;
  margin: 0;
  padding: 12px 14px;
  font-size: 12px;
  line-height: 1.2;
  white-space: pre;
  overflow: auto;
  color: #ccc;
  background: #000;
  -webkit-overflow-scrolling: touch;
}
</style>
