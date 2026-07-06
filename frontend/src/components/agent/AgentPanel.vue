<template>
  <div class="agent-backdrop" :class="{ open }" @click.self="$emit('close')">
    <div class="agent-panel" :class="{ open }">
      <div class="agent-header">
        <h2>{{ t('agent.title') }}</h2>
        <button class="agent-close" @click="$emit('close')"><X :size="14" /></button>
      </div>

      <div class="agent-body" v-if="sessions.length === 0 && !loading">
        <p class="agent-empty">{{ t('agent.noSessions') }}</p>
      </div>

      <div class="agent-body" v-else-if="loading">
        <p class="agent-empty">{{ t('app.loading') }}</p>
      </div>

      <div class="agent-layout" v-else>
        <!-- Left: session list -->
        <div class="agent-session-list">
          <div
            v-for="s in sessions"
            :key="s.paneId"
            class="agent-session-item"
            :class="{ active: s.paneId === selectedPaneId }"
            @click="selectPane(s.paneId)"
          >
            <div class="agent-session-info">
              <span class="agent-session-id">{{ s.title }}</span>
              <span class="agent-session-shell">{{ s.shellType }}</span>
            </div>
            <div class="agent-session-cwd" :title="s.cwd">{{ s.cwd || '~' }}</div>
          </div>
        </div>

        <!-- Right: command area -->
        <div class="agent-command-area">
          <template v-if="!selectedPaneId">
            <p class="agent-empty">{{ t('agent.noSelection') }}</p>
          </template>
          <template v-else>
            <!-- Pane info bar -->
            <div class="agent-pane-info">
              <span class="agent-pane-label">{{ selectedPane?.shellType }}</span>
              <span class="agent-pane-label agent-pane-cwd">{{ selectedPane?.cwd }}</span>
              <span class="agent-pane-id">{{ selectedPaneId.slice(0, 12) }}...</span>
            </div>

            <!-- Command input -->
            <div class="agent-input-row">
              <input
                v-model="currentCommand"
                type="text"
                class="agent-input"
                :placeholder="t('agent.commandPlaceholder')"
                @keydown.enter.prevent="doRun"
                :disabled="running"
              />
              <button
                class="agent-run-btn"
                :class="{ running }"
                @click="doRun"
                :disabled="running || !currentCommand.trim()"
              >
                {{ running ? '...' : t('agent.run') }}
              </button>
            </div>

            <!-- Quick action buttons -->
            <div class="agent-quick-actions" v-if="!running">
              <button class="agent-quick-btn" @click="sendQuick('y\r')">y</button>
              <button class="agent-quick-btn" @click="sendQuick('n\r')">n</button>
              <button class="agent-quick-btn" @click="sendQuick('yes\r')">yes</button>
              <button class="agent-quick-btn" @click="sendQuick('no\r')">no</button>
              <button class="agent-quick-btn" @click="sendQuick('continue\r')">continue</button>
              <button class="agent-quick-btn" @click="sendQuick('\x03')">Ctrl+C</button>
            </div>

            <!-- Results list -->
            <div class="agent-results" ref="resultsRef">
              <div
                v-for="r in results"
                :key="r.id"
                class="agent-result-card"
                :class="{ success: r.exitCode === 0, fail: r.exitCode !== 0 }"
              >
                <div class="agent-result-meta">
                  <span class="agent-result-cmd">$ {{ r.command }}</span>
                  <span class="agent-result-status" :class="r.exitCode === 0 ? 'ok' : 'err'">
                    {{ r.exitCode === 0 ? '✓' : '✗' }} exit {{ r.exitCode }}
                  </span>
                  <span class="agent-result-duration">{{ fmtDuration(r.duration) }}</span>
                  <button class="agent-result-copy" @click="copyText(r.stdout)" :title="t('agent.copy')">
                    <Clipboard :size="12" />
                  </button>
                </div>
                <pre v-if="r.stdout" class="agent-result-stdout">{{ r.stdout }}</pre>
              </div>
              <p v-if="results.length === 0" class="agent-empty agent-empty-small">
                {{ t('agent.noResults') }}
              </p>
            </div>
          </template>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'
import { X, Clipboard } from 'lucide-vue-next'
import { useI18n } from '../../composables/useI18n'
import { useAgentPanel } from '../../composables/useAgentPanel'
import { authFetch, apiUrl } from '../../composables/apiBase'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const { t } = useI18n()
const {
  sessions,
  selectedPaneId,
  selectedPane,
  results,
  loading,
  running,
  currentCommand,
  fetchSessions,
  runCommand,
  selectPane,
} = useAgentPanel()

const resultsRef = ref<HTMLElement>()

async function doRun() {
  const cmd = currentCommand.value.trim()
  if (!cmd || running.value) return
  currentCommand.value = ''
  await runCommand(cmd)
  nextTick(() => {
    resultsRef.value?.scrollTo({ top: 0, behavior: 'smooth' })
  })
}

async function sendQuick(text: string) {
  if (!selectedPaneId.value) return
  try {
    await authFetch(apiUrl('/api/agent/send'), {
      method: 'POST',
      body: JSON.stringify({ command: text, pane_id: selectedPaneId.value }),
    })
  } catch {
    // ignore
  }
}

function fmtDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
  return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    // fallback ignored
  }
}

watch(
  () => props.open,
  (v) => {
    if (v) {
      loading.value = true
      fetchSessions().finally(() => { loading.value = false })
    }
  }
)
</script>

<style scoped>
.agent-backdrop {
  position: fixed;
  inset: 0;
  z-index: 900;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  justify-content: center;
  align-items: center;
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.2s;
}
.agent-backdrop.open {
  opacity: 1;
  pointer-events: auto;
}
.agent-panel {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  width: 90vw;
  max-width: 900px;
  height: 80vh;
  max-height: 700px;
  display: flex;
  flex-direction: column;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
  transform: scale(0.95) translateY(10px);
  transition: transform 0.2s;
}
.agent-panel.open {
  transform: scale(1) translateY(0);
}
.agent-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
}
.agent-header h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}
.agent-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fg-muted);
  cursor: pointer;
}
.agent-close:hover {
  background: var(--bg-hover);
}
.agent-empty {
  text-align: center;
  color: var(--fg-muted);
  padding: 40px 20px;
  font-size: 14px;
}
.agent-empty-small {
  padding: 16px;
  font-size: 12px;
}
.agent-layout {
  display: flex;
  flex: 1;
  overflow: hidden;
}
.agent-session-list {
  width: 200px;
  min-width: 160px;
  border-right: 1px solid var(--border);
  overflow-y: auto;
  padding: 4px 0;
  flex-shrink: 0;
}
.agent-session-item {
  padding: 8px 12px;
  cursor: pointer;
  border-left: 3px solid transparent;
  transition: background 0.1s;
}
.agent-session-item:hover {
  background: var(--bg-hover);
}
.agent-session-item.active {
  background: var(--bg-active);
  border-left-color: var(--accent);
}
.agent-session-info {
  display: flex;
  align-items: center;
  gap: 6px;
}
.agent-session-id {
  font-size: 13px;
  font-weight: 500;
}
.agent-session-shell {
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--bg-input);
  color: var(--fg-muted);
}
.agent-session-cwd {
  font-size: 11px;
  color: var(--fg-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin-top: 2px;
}
.agent-command-area {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.agent-pane-info {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  flex-shrink: 0;
}
.agent-pane-label {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--bg-input);
}
.agent-pane-cwd {
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.agent-pane-id {
  font-size: 10px;
  color: var(--fg-muted);
  font-family: monospace;
  margin-left: auto;
}
.agent-input-row {
  display: flex;
  gap: 6px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.agent-input {
  flex: 1;
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg-input);
  color: var(--fg);
  font-size: 13px;
  font-family: inherit;
  outline: none;
}
.agent-input:focus {
  border-color: var(--accent);
}
.agent-run-btn {
  padding: 6px 16px;
  border: none;
  border-radius: 6px;
  background: var(--accent);
  color: #fff;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  white-space: nowrap;
}
.agent-run-btn:hover:not(:disabled) {
  opacity: 0.9;
}
.agent-run-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.agent-run-btn.running {
  background: var(--fg-muted);
}
.agent-quick-actions {
  display: flex;
  gap: 4px;
  padding: 4px 12px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  flex-wrap: wrap;
}
.agent-quick-btn {
  padding: 2px 8px;
  border: 1px solid var(--border);
  border-radius: 4px;
  background: var(--bg);
  color: var(--fg);
  font-size: 11px;
  cursor: pointer;
}
.agent-quick-btn:hover {
  background: var(--bg-hover);
}
.agent-results {
  flex: 1;
  overflow-y: auto;
  padding: 8px 12px;
}
.agent-result-card {
  margin-bottom: 8px;
  padding: 8px 10px;
  border-radius: 6px;
  border: 1px solid var(--border);
  background: var(--bg);
}
.agent-result-card.success {
  border-left: 3px solid #22c55e;
}
.agent-result-card.fail {
  border-left: 3px solid #ef4444;
}
.agent-result-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  margin-bottom: 4px;
}
.agent-result-cmd {
  font-family: monospace;
  font-weight: 500;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.agent-result-status {
  font-size: 11px;
  font-weight: 600;
}
.agent-result-status.ok {
  color: #22c55e;
}
.agent-result-status.err {
  color: #ef4444;
}
.agent-result-duration {
  color: var(--fg-muted);
  font-size: 11px;
}
.agent-result-copy {
  display: flex;
  padding: 2px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--fg-muted);
  cursor: pointer;
}
.agent-result-copy:hover {
  background: var(--bg-hover);
  color: var(--fg);
}
.agent-result-stdout {
  font-size: 12px;
  font-family: monospace;
  white-space: pre-wrap;
  word-break: break-all;
  margin: 4px 0 0;
  color: var(--fg);
  line-height: 1.4;
  max-height: 300px;
  overflow-y: auto;
}
</style>