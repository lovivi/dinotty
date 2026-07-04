import { ref } from 'vue'
import type { TerminalInstance } from './useTerminal'
import { useHistory } from './useHistory'

/**
 * Inline ghost-text autocomplete, modelled after fish / zsh-autosuggestions
 * with arrow-key navigation (Warp-style).
 *
 *  - The server ranks history entries by frequency then recency (see
 *    `src/history.rs::query()`); the client trusts that ordering and keeps
 *    the top suggestions.
 *  - Suggestions 1..N are shown in a dropdown; suggestion 0 is also painted
 *    inline as ghost text after the cursor.
 *  - Accept:        → / End (fish/zsh convention; we deliberately do NOT
 *    bind Tab because shell readline uses Tab for filename completion
 *    and stealing it confuses muscle memory).
 *  - Cycle:         ↓ / ↑ (Warp/Fish convention; Ctrl+F / Shift+Tab were
 *    tried but conflict with readline key bindings and editor habits).
 *  - Dismiss:       Esc.
 *  - Cycle keys are intercepted ONLY while the dropdown is visible; when
 *    no suggestion is showing they fall through to the shell so users
 *    can still use ↑/↓ for shell history navigation.
 *
 * Multi-pane safety: this composable is called inside each `TerminalPane`'s
 * `setup()`, so every pane gets its own instance. `bind()` is called once
 * per pane and never hoisted.
 */
export function useAutocomplete() {
  const { fetchSuggestions } = useHistory()

  const visible = ref(false)
  /** Ordered suggestions, index 0 is the inline ghost. Server order is
   *  frequency desc then recency desc; the client re-sorts prefix matches
   *  ahead of contains matches. */
  const suggestions = ref<string[]>([])
  /** Which suggestion in the dropdown is highlighted. Always 0 means the
   *  inline ghost; higher values come from Ctrl+F / Shift+Tab / mouse hover. */
  const selectedIdx = ref(0)
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

  /** Number of dropdown rows we show below the inline ghost. Server returns
   *  up to 20; the dropdown caps at this so it never covers half the screen. */
  const DROPDOWN_MAX = 8

  /** Extract user input from a terminal line, stripping the shell prompt
   *  (which typically ends with "$ ", "# " or "% ") from the start. */
  function extractPrefix(text: string, cursorX: number): string {
    const beforeCursor = cursorX >= text.length ? text : text.substring(0, cursorX)
    const promptEnd = Math.max(
      beforeCursor.lastIndexOf('$ '),
      beforeCursor.lastIndexOf('# '),
      beforeCursor.lastIndexOf('% '),
    )
    const start = promptEnd >= 0 ? promptEnd + 2 : 0
    return beforeCursor.substring(start).trim()
  }

  /** Exposed for tests; production code reads this off the terminal instance. */
  let onBeforeSendHook: ((data: string) => boolean) | null = null

  function bind(term: TerminalInstance) {
    terminal = term
    onBeforeSendHook = onBeforeSend
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

    // → arrow (CSI C / SS3 C) — accept current selection
    if (data === '\x1b[C' || data === '\x1bOC') {
      if (visible.value && currentSelected()) {
        accept()
        return true
      }
      return false
    }

    // ↓ arrow (CSI B / SS3 B) — cycle to next suggestion. Only intercepted
    // when the dropdown is visible so the keystroke still reaches the shell
    // (history navigation) when no suggestion is up.
    if (data === '\x1b[B' || data === '\x1bOB') {
      if (visible.value && suggestions.value.length > 1) {
        selectNext()
        return true
      }
      return false
    }

    // ↑ arrow (CSI A / SS3 A) — cycle to previous suggestion. Same gating
    // as ↓ above.
    if (data === '\x1b[A' || data === '\x1bOA') {
      if (visible.value && suggestions.value.length > 1) {
        selectPrev()
        return true
      }
      return false
    }

    // End (CSI F / SS3 F) — fish/zsh convention: jump to end of line and
    // accept the current selection.
    if (data === '\x1b[F' || data === '\x1bOF') {
      if (visible.value && currentSelected()) {
        accept()
        return true
      }
      return false
    }

    // Esc — dismiss without accepting
    if (data === '\x1b') {
      if (visible.value) {
        dismiss()
        return true
      }
      return false
    }

    // Enter — accept and submit; dismiss the dropdown
    if (data === '\r') {
      if (visible.value) dismiss()
      return false
    }

    // Any other keystroke — let it through to the shell, then re-evaluate
    // the prefix. `updateFromBuffer()` hides the ghost when prefix is empty
    // and refetches suggestions when the prefix changed.
    setTimeout(() => updateFromBuffer(), 0)
    return false
  }

  /** The suggestion currently highlighted (inline ghost if 0, dropdown row otherwise). */
  function currentSelected(): string {
    return suggestions.value[selectedIdx.value] ?? ''
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
    const prefix = extractPrefix(text, cursorX)

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

  /** Extract user input prefix, stripping the shell prompt from the line. */
  function _extractPrefix(text: string, cursorX: number): string {
    const beforeCursor = cursorX >= text.length ? text : text.substring(0, cursorX)
    // Shell prompts typically end with "$ ", "# " (bash/sh) or "% " (zsh).
    // Strip everything before the LAST occurrence of these markers.
    const promptEnd = Math.max(
      beforeCursor.lastIndexOf('$ '),
      beforeCursor.lastIndexOf('# '),
      beforeCursor.lastIndexOf('% '),
    )
    const start = promptEnd >= 0 ? promptEnd + 2 : 0
    return beforeCursor.substring(start).trim()
  }

  function runFetch(prefix: string) {
    void (async () => {
      const items = await fetchSuggestions(prefix)
      if (lastPrefix !== prefix) return
      // Server returns Top N ranked by (frequency desc, recency desc). Split
      // into startsWith + contains-fallback so prefix-aligned matches always
      // outrank fuzzy ones.
      const cmds = items.map((it) => it.command)
      const starts = cmds.filter((c) => c.length > prefix.length && c.startsWith(prefix))
      const fallback = cmds.filter(
        (c) => c.length > prefix.length && !c.startsWith(prefix) && c.includes(prefix)
      )
      const ranked = [...starts, ...fallback]
      const top = ranked.slice(0, DROPDOWN_MAX + 1) // ghost + 8 dropdown rows
      if (top.length > 0) {
        suggestions.value = top
        selectedIdx.value = 0
        visible.value = true
      } else {
        visible.value = false
      }
    })()
  }

  /** Highlight the next suggestion in the dropdown. Wraps around. */
  function selectNext() {
    if (suggestions.value.length === 0) return
    selectedIdx.value = (selectedIdx.value + 1) % suggestions.value.length
  }

  /** Highlight the previous suggestion in the dropdown. Wraps around. */
  function selectPrev() {
    if (suggestions.value.length === 0) return
    selectedIdx.value =
      (selectedIdx.value - 1 + suggestions.value.length) % suggestions.value.length
  }

  /** Mouse hover handler — called by `InlineAutocomplete` via emit. */
  function setSelected(idx: number) {
    if (idx < 0 || idx >= suggestions.value.length) return
    selectedIdx.value = idx
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
    if (!terminal) return
    const suggestion = currentSelected()
    const prefix = typedPrefix.value
    if (!suggestion || suggestion.length <= prefix.length) return
    const completion = suggestion.substring(prefix.length)
    terminal.sendData(completion)
    dismiss()
  }

  function dismiss() {
    visible.value = false
    suggestions.value = []
    selectedIdx.value = 0
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
    suggestions,
    selectedIdx,
    typedPrefix,
    cursorPixelX,
    cursorPixelY,
    cursorFontSize,
    cursorFontFamily,
    onBeforeSend: onBeforeSend,
    bind,
    unbind,
    accept,
    dismiss,
    selectNext,
    selectPrev,
    setSelected,
  }
}
