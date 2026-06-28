import { ref } from 'vue'
import type { TerminalInstance } from './useTerminal'
import { useHistory } from './useHistory'

export function useAutocomplete() {
  const { fetchSuggestions } = useHistory()

  const visible = ref(false)
  const currentSuggestion = ref('')
  const typedPrefix = ref('')
  const cursorPixelX = ref(0)
  const cursorPixelY = ref(0)
  let terminal: TerminalInstance | null = null
  let fetchTimer: ReturnType<typeof setTimeout> | null = null
  let lastPrefix = ''

  function bind(term: TerminalInstance) {
    terminal = term
    term.onBeforeSend = onBeforeSend
  }

  function onBeforeSend(data: string): boolean {
    if (!terminal) return false

    if (terminal.isMouseModeEnabled()) {
      if (visible.value) dismiss()
      return false
    }

    if (data === '\t') {
      if (visible.value && currentSuggestion.value) {
        accept()
        return true
      }
      return false
    }

    if (data === '\x1b') {
      if (visible.value) {
        dismiss()
        return true
      }
      return false
    }

    if (data === '\r') {
      if (visible.value) dismiss()
      return false
    }

    setTimeout(() => updateFromBuffer(), 0)
    return false
  }

  function updateFromBuffer() {
    const xt = terminal?.xterm
    if (!xt) return

    const cursorY = xt.buffer.active.cursorY
    const cursorX = xt.buffer.active.cursorX
    const line = xt.buffer.active.getLine(cursorY)
    if (!line) {
      visible.value = false
      return
    }

    const text = line.translateToString()
    const prefix = text.substring(0, cursorX).trim()

    if (prefix.length < 2) {
      visible.value = false
      lastPrefix = ''
      return
    }

    typedPrefix.value = prefix
    updateCursorPixel()

    if (prefix === lastPrefix) return
    lastPrefix = prefix

    if (fetchTimer) clearTimeout(fetchTimer)
    fetchTimer = setTimeout(async () => {
      const items = await fetchSuggestions(prefix)
      // Only use prefix matches for ghost text UX
      const match = items.find((item) => item.command.startsWith(prefix))
      if (match && lastPrefix === prefix) {
        currentSuggestion.value = match.command
        visible.value = true
      } else {
        visible.value = false
      }
    }, 100)
  }

  function updateCursorPixel() {
    const xt = terminal?.xterm
    if (!xt) return
    try {
      const cursorX = xt.buffer.active.cursorX
      const cursorY = xt.buffer.active.cursorY
      const core = (xt as any)._core
      const dims = core?._renderService?.dimensions
      const cellW = dims?.css?.cell?.width ?? 9
      const cellH = dims?.css?.cell?.height ?? 18
      const screenEl = xt.element?.querySelector('.xterm-screen')
      if (!screenEl) return
      const screenRect = screenEl.getBoundingClientRect()
      cursorPixelX.value = screenRect.left + cursorX * cellW
      cursorPixelY.value = screenRect.top + cursorY * cellH
    } catch {
      // xterm.js internal API may change; degrade gracefully
    }
  }

  function accept() {
    if (!terminal || !currentSuggestion.value) return
    const suggestion = currentSuggestion.value
    const prefix = typedPrefix.value
    const completion = suggestion.substring(prefix.length)
    if (completion) {
      terminal.sendData(completion)
    }
    dismiss()
  }

  function dismiss() {
    visible.value = false
    currentSuggestion.value = ''
    lastPrefix = ''
  }

  function unbind() {
    if (terminal) {
      terminal.onBeforeSend = null
      terminal = null
    }
    if (fetchTimer) {
      clearTimeout(fetchTimer)
      fetchTimer = null
    }
    dismiss()
  }

  return {
    visible,
    currentSuggestion,
    typedPrefix,
    cursorPixelX,
    cursorPixelY,
    bind,
    unbind,
    accept,
    dismiss,
  }
}
