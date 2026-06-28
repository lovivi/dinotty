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
  const cursorFontSize = ref(14)
  const cursorFontFamily = ref('monospace')
  let terminal: TerminalInstance | null = null
  let fetchTimer: ReturnType<typeof setTimeout> | null = null
  let lastPrefix = ''
  let cachedCellW = 0
  let cachedCellH = 0
  let rafHandle: number | null = null
  let cleanupViewport: (() => void) | null = null

  function bind(term: TerminalInstance) {
    terminal = term
    term.onBeforeSend = onBeforeSend
    primeHistory()
    attachViewportListeners()
  }

  function primeHistory() {
    void fetchSuggestions('')
  }

  function onBeforeSend(data: string): boolean {
    if (!terminal) return false

    if (terminal.isMouseModeEnabled()) {
      if (visible.value) dismiss()
      return false
    }

    // Right arrow (CSI C / SS3 C) — accept current suggestion
    if (data === '\x1b[C' || data === '\x1bOC') {
      if (visible.value && currentSuggestion.value) {
        accept()
        return true
      }
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

    if (prefix.length < 1) {
      visible.value = false
      lastPrefix = ''
      return
    }

    typedPrefix.value = prefix
    updateCursorPixel()

    if (prefix === lastPrefix) return
    lastPrefix = prefix

    if (fetchTimer) clearTimeout(fetchTimer)
    fetchTimer = setTimeout(() => runFetch(prefix), 50)
  }

  function runFetch(prefix: string) {
    void (async () => {
      const items = await fetchSuggestions(prefix)
      if (lastPrefix !== prefix) return
      const starts = items
        .filter((it) => it.command.length > prefix.length && it.command.startsWith(prefix))
        .sort((a, b) => a.command.length - b.command.length || b.frequency - a.frequency)
      const fallback = items
        .filter(
          (it) => it.command.length > prefix.length && !it.command.startsWith(prefix) && it.command.includes(prefix)
        )
        .sort((a, b) => a.command.length - b.command.length || b.frequency - a.frequency)
      const match = starts[0] ?? fallback[0]
      if (match) {
        currentSuggestion.value = match.command
        visible.value = true
      } else {
        visible.value = false
      }
    })()
  }

  function measureCell() {
    const xt = terminal?.xterm
    if (!xt?.element) return null
    try {
      const core = (xt as any)._core
      const dims = core?._renderService?.dimensions
      const cssCell = dims?.css?.cell
      if (cssCell?.width && cssCell?.height) {
        return { width: cssCell.width, height: cssCell.height }
      }
    } catch {
      /* fall through to DOM measurement */
    }
    const rowsEl = xt.element.querySelector('.xterm-rows > div') as HTMLElement | null
    const screenEl = xt.element.querySelector('.xterm-screen') as HTMLElement | null
    if (!rowsEl || !screenEl) return null
    const rowRect = rowsEl.getBoundingClientRect()
    if (!rowRect.height) return null
    const charEl = screenEl.querySelector('span') as HTMLElement | null
    const charWidth = charEl?.getBoundingClientRect().width ?? rowRect.height * 0.6
    return { width: charWidth, height: rowRect.height }
  }

  function updateCursorPixel() {
    const xt = terminal?.xterm
    if (!xt) return
    try {
      const cursorX = xt.buffer.active.cursorX
      const cursorY = xt.buffer.active.cursorY
      const viewportY = xt.buffer.active.viewportY ?? 0
      const cell = measureCell()
      if (cell) {
        cachedCellW = cell.width
        cachedCellH = cell.height
      }
      const cellW = cachedCellW || 9
      const cellH = cachedCellH || 18
      const screenEl = xt.element?.querySelector('.xterm-screen') as HTMLElement | null
      if (!screenEl) return
      const screenRect = screenEl.getBoundingClientRect()
      cursorPixelX.value = screenRect.left + cursorX * cellW
      cursorPixelY.value = screenRect.top + (cursorY - viewportY) * cellH
      cursorFontSize.value = xt.options.fontSize ?? 14
      cursorFontFamily.value = xt.options.fontFamily ?? 'monospace'
    } catch {
      // xterm.js internal API may change; degrade gracefully
    }
  }

  function schedulePixelRefresh() {
    if (rafHandle !== null) return
    rafHandle = requestAnimationFrame(() => {
      rafHandle = null
      if (visible.value) updateCursorPixel()
    })
  }

  function attachViewportListeners() {
    detachViewportListeners()
    const xt = terminal?.xterm
    const viewport = xt?.element?.querySelector('.xterm-viewport') as HTMLElement | null
    if (!viewport) return
    const onScroll = () => schedulePixelRefresh()
    const onResize = () => schedulePixelRefresh()
    viewport.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', onResize)
    cleanupViewport = () => {
      viewport.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', onResize)
    }
  }

  function detachViewportListeners() {
    if (cleanupViewport) {
      cleanupViewport()
      cleanupViewport = null
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
    if (rafHandle !== null) {
      cancelAnimationFrame(rafHandle)
      rafHandle = null
    }
    detachViewportListeners()
    dismiss()
  }

  return {
    visible,
    currentSuggestion,
    typedPrefix,
    cursorPixelX,
    cursorPixelY,
    cursorFontSize,
    cursorFontFamily,
    bind,
    unbind,
    accept,
    dismiss,
  }
}
