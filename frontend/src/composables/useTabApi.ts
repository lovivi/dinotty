import { authFetch, apiUrl } from './apiBase'
import type { SyncTabInfo } from '../types/protocol'

export interface CreateTabResult {
  tab_id: string
  pane_id: string
  layout: any
  shell_profile_id?: string
  shell_profile_name?: string
  group_id?: string | null
  workspace_roots?: string[]
}

export interface SplitPaneResult {
  new_pane_id: string
  layout: any
}

export interface ClosePaneResult {
  ok: boolean
  tab_closed: boolean
  layout?: any
  active_pane_id?: string
}

export interface ListTabsResult {
  tabs: SyncTabInfo[]
  active_pane_id: string | null
}

export async function apiListTabs(): Promise<ListTabsResult> {
  const res = await authFetch(apiUrl('/api/tabs'))
  if (!res.ok) throw new Error(`list tabs failed: ${res.status}`)
  return res.json()
}

export interface RestoredPane {
  pane_id: string
  tab_id: string
  cwd?: string | null
  shell_profile_id?: string | null
  shell_profile_name?: string | null
  output_tail: string[]
  recent_commands: string[]
}

export interface WorksiteState {
  version: number
  updated_at: number
  active_pane_id?: string | null
  panes: RestoredPane[]
}

export async function apiGetRestoreState(): Promise<WorksiteState> {
  const res = await authFetch(apiUrl('/api/restore-state'))
  if (!res.ok) throw new Error(`get restore state failed: ${res.status}`)
  return res.json()
}

export async function apiClearRestoreState(): Promise<void> {
  const res = await authFetch(apiUrl('/api/restore-state'), { method: 'DELETE' })
  if (!res.ok && res.status !== 204) throw new Error(`clear restore state failed: ${res.status}`)
}

export interface ShellProfile {
  id: string
  name: string
  kind: 'unix' | 'wsl' | 'powershell' | 'cmd' | 'custom'
  command: string
  args: string[]
  wsl_distro?: string | null
  is_default: boolean
  source: string
}

export interface ShellProfilesResult {
  profiles: ShellProfile[]
  default_profile_id?: string | null
}

export async function apiUpdateTabMeta(
  tabId: string,
  meta: { group_id?: string | null; workspace_roots?: string[] }
): Promise<void> {
  const body: Record<string, unknown> = {}
  if (meta.group_id !== undefined) body.group_id = meta.group_id
  if (meta.workspace_roots !== undefined) body.workspace_roots = meta.workspace_roots
  const res = await authFetch(apiUrl(`/api/tabs/${tabId}/meta`), {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error(`update tab meta failed: ${res.status}`)
}

export async function apiListShellProfiles(): Promise<ShellProfilesResult> {
  const res = await authFetch(apiUrl('/api/shell/profiles'))
  if (!res.ok) throw new Error(`list shell profiles failed: ${res.status}`)
  return res.json()
}

export interface CreateTabOptions {
  profile_id?: string
  group_id?: string | null
  workspace_roots?: string[]
  cwd_ref?: unknown
}

export async function apiCreateTab(options?: CreateTabOptions): Promise<CreateTabResult> {
  const init: RequestInit = { method: 'POST' }
  if (options) {
    init.headers = { 'Content-Type': 'application/json' }
    init.body = JSON.stringify(options)
  }
  const res = await authFetch(apiUrl('/api/tabs'), init)
  if (!res.ok) throw new Error(`create tab failed: ${res.status}`)
  return res.json()
}

export async function apiCloseTab(tabId: string): Promise<void> {
  const res = await authFetch(apiUrl(`/api/tabs/${tabId}`), { method: 'DELETE' })
  if (!res.ok) throw new Error(`close tab failed: ${res.status}`)
}

export async function apiSplitPane(
  tabId: string,
  paneId: string,
  direction: string
): Promise<SplitPaneResult> {
  const res = await authFetch(apiUrl(`/api/tabs/${tabId}/pane`), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pane_id: paneId, direction }),
  })
  if (!res.ok) throw new Error(`split pane failed: ${res.status}`)
  return res.json()
}

export async function apiClosePane(tabId: string, paneId: string): Promise<ClosePaneResult> {
  const res = await authFetch(apiUrl(`/api/tabs/${tabId}/pane/${paneId}`), { method: 'DELETE' })
  if (!res.ok) throw new Error(`close pane failed: ${res.status}`)
  return res.json()
}

export async function apiActivatePane(tabId: string, paneId: string): Promise<void> {
  const res = await authFetch(apiUrl(`/api/tabs/${tabId}/pane/${paneId}/activate`), {
    method: 'PUT',
  })
  if (!res.ok) throw new Error(`activate pane failed: ${res.status}`)
}

export async function apiUpdateLayout(
  tabId: string,
  layout: any,
  activePaneId: string
): Promise<void> {
  const res = await authFetch(apiUrl(`/api/tabs/${tabId}/layout`), {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ layout, active_pane_id: activePaneId }),
  })
  if (!res.ok) throw new Error(`update layout failed: ${res.status}`)
}
