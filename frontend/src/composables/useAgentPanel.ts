import { ref, computed } from 'vue'
import { authFetch, apiUrl } from './apiBase'
import { useToast, TYPE } from 'vue-toastification'
import { useI18n } from './useI18n'

export interface PaneSummary {
  paneId: string
  tabId: string
  shellType: string
  cwd: string
  title: string
}

export interface RunResult {
  id: string
  command: string
  exitCode: number
  stdout: string
  duration: number
  timestamp: number
}

export function useAgentPanel() {
  const sessions = ref<PaneSummary[]>([])
  const selectedPaneId = ref<string | null>(null)
  const results = ref<RunResult[]>([])
  const loading = ref(false)
  const running = ref(false)
  const currentCommand = ref('')
  const screenContent = ref('')

  const selectedPane = computed(() =>
    sessions.value.find((s) => s.paneId === selectedPaneId.value) ?? null
  )

  async function fetchSessions() {
    try {
      const res = await authFetch(apiUrl('/api/tabs'))
      if (!res.ok) return
      const data = await res.json()
      const tabs: any[] = data.tabs ?? []
      const paneList: PaneSummary[] = []
      for (const tab of tabs) {
        const leaves = collectLeafPanes(tab.layout)
        for (const paneId of leaves) {
          paneList.push({
            paneId,
            tabId: tab.tab_id,
            shellType: tab.shell_profile_name ?? 'terminal',
            cwd: tab.workspace_roots?.[0] ?? '',
            title: tab.title ?? paneId.slice(0, 8),
          })
        }
      }
      sessions.value = paneList
    } catch (e) {
      console.warn('[agent] Failed to fetch sessions:', e)
    }
  }

  async function readPane(paneId: string) {
    try {
      const res = await authFetch(apiUrl(`/api/agent/read?pane_id=${encodeURIComponent(paneId)}`))
      if (!res.ok) return
      const data = await res.json()
      screenContent.value = data.lines?.join('\n') ?? ''
      // Update cwd from response
      if (data.cwd) {
        const pane = sessions.value.find((s) => s.paneId === paneId)
        if (pane) pane.cwd = data.cwd
      }
    } catch (e) {
      console.warn('[agent] Failed to read pane:', e)
    }
  }

  async function runCommand(command: string): Promise<void> {
    if (!selectedPaneId.value || !command) return
    running.value = true
    try {
      const res = await authFetch(apiUrl('/api/agent/run'), {
        method: 'POST',
        body: JSON.stringify({
          command,
          pane_id: selectedPaneId.value,
          timeout: 300_000,
        }),
      })
      if (!res.ok) {
        const err = await res.json().catch(() => ({ error: { message: 'Request failed' } }))
        const toast = useToast()
        toast(err.error?.message ?? 'Command failed', { type: TYPE.ERROR })
        return
      }
      const data = await res.json()
      results.value.unshift({
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        command,
        exitCode: data.exit_code,
        stdout: data.stdout,
        duration: data.duration,
        timestamp: Date.now(),
      })
      // Keep last 20 results
      if (results.value.length > 20) results.value.length = 20
    } catch (e) {
      const toast = useToast()
      toast('Failed to run command', { type: TYPE.ERROR })
      console.warn('[agent] Run error:', e)
    } finally {
      running.value = false
    }
  }

  function selectPane(paneId: string) {
    selectedPaneId.value = paneId
    readPane(paneId)
  }

  function clearResults() {
    results.value = []
  }

  return {
    sessions,
    selectedPaneId,
    selectedPane,
    results,
    loading,
    running,
    currentCommand,
    screenContent,
    fetchSessions,
    readPane,
    runCommand,
    selectPane,
    clearResults,
  }
}

function collectLeafPanes(layout: any): string[] {
  const ids: string[] = []
  function walk(node: any) {
    if (!node) return
    if (node.type === 'leaf') {
      if (node.paneId) ids.push(node.paneId)
    } else if (node.type === 'split' && node.children) {
      for (const child of node.children) walk(child)
    }
  }
  walk(layout)
  return ids
}
