function isTauri(): boolean {
  const w = window as any
  return !!(w.__TAURI_INTERNALS__?.invoke || w.__TAURI__?.core?.invoke)
}

function tauriInvoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  const tauri = (window as any).__TAURI__
  const invoke =
    tauri?.core?.invoke ??
    ((c: string, a?: object) => (window as any).__TAURI_INTERNALS__.invoke(c, a ?? {}))
  return invoke(cmd, args ?? {})
}

export async function copyToClipboard(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text)
    return
  } catch {
    // Fall through to fallback
  }

  // Secondary: execCommand on a temporary textarea
  try {
    const ta = document.createElement('textarea')
    ta.value = text
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    ta.style.pointerEvents = 'none'
    document.body.appendChild(ta)
    ta.focus()
    ta.select()
    const ok = document.execCommand('copy')
    ta.remove()
    if (ok) return
  } catch {
    // execCommand may throw
  }

  // Tertiary: Tauri clipboard plugin
  if (isTauri()) {
    try {
      await tauriInvoke('plugin:clipboard-manager|write_text', { text })
    } catch {
      // Clipboard plugin not available
    }
  }
}

export async function pasteFromClipboard(): Promise<string> {
  try {
    return await navigator.clipboard.readText()
  } catch {
    // clipboard.readText denied — try Tauri plugin
  }
  if (isTauri()) {
    try {
      return (await tauriInvoke('plugin:clipboard-manager|read_text')) as string
    } catch {
      // Clipboard plugin not available
    }
  }
  return ''
}
