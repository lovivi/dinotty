/**
 * Tests for `useAutocomplete`, the fish/zsh-style ghost-text + dropdown
 * command suggestion engine. Mocks `useHistory` so no network calls happen.
 */
import { describe, it, expect, beforeEach, vi } from 'vitest'

// Mock useHistory so we control the suggestion set without hitting the server.
const fetchSuggestionsMock = vi.fn()
vi.mock('../composables/useHistory', () => ({
  useHistory: () => ({
    suggestions: { value: [] },
    fetchSuggestions: fetchSuggestionsMock,
    fetchDebounced: vi.fn(),
    deleteSuggestion: vi.fn(),
  }),
}))

// Minimal TerminalInstance stand-in: only the methods useAutocomplete touches.
function makeTerminal() {
  const sent: string[] = []
  const onBeforeSend = vi.fn()
  let bufferLine = ''
  let bufferCursor = 0
  const term = {
    isMouseModeEnabled: () => false,
    sendData: (data: string) => sent.push(data),
    onBeforeSend: null as null | ((data: string) => boolean),
    options: { fontSize: 14, fontFamily: 'monospace' },
    element: null,
    xterm: {
      buffer: {
        active: {
          cursorX: 0,
          cursorY: 0,
          viewportY: 0,
          getLine: () => ({
            translateToString: () => bufferLine,
          }),
        },
      },
      options: { fontSize: 14, fontFamily: 'monospace' },
      element: null,
    },
  } as any
  term.onBeforeSend = onBeforeSend
  return { term, sent, onBeforeSend, setBuffer(text: string, cursor: number) {
    bufferLine = text
    bufferCursor = cursor
    term.xterm.buffer.active.cursorX = cursor
  }}
}

// Re-import after mock so the composable picks up the mocked module.
import { useAutocomplete } from '../composables/useAutocomplete'

beforeEach(() => {
  fetchSuggestionsMock.mockReset()
})

describe('useAutocomplete', () => {
  it('starts hidden and with no suggestions', () => {
    const ac = useAutocomplete()
    expect(ac.visible.value).toBe(false)
    expect(ac.suggestions.value).toEqual([])
    expect(ac.selectedIdx.value).toBe(0)
  })

  it('shows startsWith suggestions ranked before contains fallback', async () => {
    // Server returns items in its rank order: frequency desc, then recency
    // desc (see src/history.rs::query). The client trusts that ordering.
    fetchSuggestionsMock.mockResolvedValue([
      { command: 'git status', frequency: 99 },
      { command: 'git push --tags', frequency: 1 },
      { command: 'cd /tmp/foo', frequency: 5 },
    ])
    const { term, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git', 3)
    ac.onBeforeSend('g')  // arbitrary trigger to call updateFromBuffer
    // Flush microtasks: fetchSuggestions is async; runFetch awaits it.
    await new Promise((r) => setTimeout(r, 80))
    expect(ac.visible.value).toBe(true)
    expect(ac.suggestions.value[0]).toBe('git status')
    expect(ac.suggestions.value[1]).toBe('git push --tags')
    // No match for prefix `git` so `cd /tmp/foo` should NOT appear.
    expect(ac.suggestions.value).not.toContain('cd /tmp/foo')
  })

  it('accept on Tab sends the missing tail to the shell', async () => {
    fetchSuggestionsMock.mockResolvedValue([
      { command: 'git status', frequency: 1 },
    ])
    const { term, sent, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git sta', 7)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 80))
    expect(ac.suggestions.value[0]).toBe('git status')
    // Tab → accept. prefix is `git sta` (trimmed from `git sta`).
    const blocked = ac.onBeforeSend('\t')
    expect(blocked).toBe(true)
    expect(sent).toEqual(['tus'])
    expect(ac.visible.value).toBe(false)
  })

  it('Esc dismisses the dropdown without accepting', async () => {
    fetchSuggestionsMock.mockResolvedValue([{ command: 'git status', frequency: 1 }])
    const { term, sent, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git', 3)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 80))
    expect(ac.visible.value).toBe(true)
    const blocked = ac.onBeforeSend('\x1b')
    expect(blocked).toBe(true)
    expect(sent).toEqual([])
    expect(ac.visible.value).toBe(false)
  })

  it('Ctrl+F cycles to the next suggestion', async () => {
    fetchSuggestionsMock.mockResolvedValue([
      { command: 'git status', frequency: 99 },
      { command: 'git stash', frequency: 50 },
      { command: 'git switch', frequency: 30 },
    ])
    const { term, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git', 3)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 80))
    expect(ac.selectedIdx.value).toBe(0)
    expect(ac.onBeforeSend('\x06')).toBe(true)
    expect(ac.selectedIdx.value).toBe(1)
    expect(ac.onBeforeSend('\x06')).toBe(true)
    expect(ac.selectedIdx.value).toBe(2)
    // Wraps around.
    expect(ac.onBeforeSend('\x06')).toBe(true)
    expect(ac.selectedIdx.value).toBe(0)
  })

  it('Shift+Tab cycles to the previous suggestion (wraps)', async () => {
    fetchSuggestionsMock.mockResolvedValue([
      { command: 'git status', frequency: 99 },
      { command: 'git stash', frequency: 50 },
    ])
    const { term, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git', 3)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 80))
    expect(ac.selectedIdx.value).toBe(0)
    expect(ac.onBeforeSend('\x1b[Z')).toBe(true)
    // Wraps from 0 back to last (length-1).
    expect(ac.selectedIdx.value).toBe(1)
  })

  it('End key accepts the current selection (fish convention)', async () => {
    fetchSuggestionsMock.mockResolvedValue([{ command: 'git status', frequency: 1 }])
    const { term, sent, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git', 3)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 80))
    const blocked = ac.onBeforeSend('\x1b[F')
    expect(blocked).toBe(true)
    expect(sent).toEqual([' status'])
  })

  it('Right arrow accepts the current selection', async () => {
    fetchSuggestionsMock.mockResolvedValue([{ command: 'git status', frequency: 1 }])
    const { term, sent, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git', 3)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 80))
    const blocked = ac.onBeforeSend('\x1b[C')
    expect(blocked).toBe(true)
    expect(sent).toEqual([' status'])
  })

  it('hides suggestions when prefix is empty', async () => {
    fetchSuggestionsMock.mockResolvedValue([{ command: 'git status', frequency: 1 }])
    const { term, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git', 3)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 80))
    expect(ac.visible.value).toBe(true)
    // User backspaces — empty prefix.
    setBuffer('', 0)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 20))
    expect(ac.visible.value).toBe(false)
  })

  it('does not accept when there are no suggestions', () => {
    const { term, sent, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('x', 1)
    // Nothing fetched yet.
    expect(ac.visible.value).toBe(false)
    expect(ac.onBeforeSend('\t')).toBe(false)
    expect(sent).toEqual([])
  })

  it('setSelected from mouse hover updates the highlighted index', async () => {
    fetchSuggestionsMock.mockResolvedValue([
      { command: 'git status', frequency: 99 },
      { command: 'git stash', frequency: 50 },
    ])
    const { term, setBuffer } = makeTerminal()
    const ac = useAutocomplete()
    ac.bind(term)
    setBuffer('git', 3)
    ac.onBeforeSend('x')
    await new Promise((r) => setTimeout(r, 80))
    expect(ac.selectedIdx.value).toBe(0)
    ac.setSelected(1)
    expect(ac.selectedIdx.value).toBe(1)
    // Out-of-range is ignored.
    ac.setSelected(99)
    expect(ac.selectedIdx.value).toBe(1)
  })
})