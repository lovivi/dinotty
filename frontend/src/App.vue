<template>
  <SetupPage v-if="!authenticated && needsSetup" @success="onLoginSuccess" />
  <LoginPage v-else-if="!authenticated" @success="onLoginSuccess" />
  <div v-else id="app-root">
    <TabBar
      :tabs="filteredTabList"
      :active-pane-id="activePaneId"
      :indicators="notif.unreadByPane"
      :plugins="pluginList"
      :can-broadcast="canBroadcast"
      :broadcast-active="isBroadcastActive"
      :is-mobile="isMobile"
      :current-tab-title="currentTabTitle"
      :current-tab-index="currentTabIndex"
      :shell-profiles="shellProfiles"
      :default-shell-profile-id="defaultShellProfileId"
      :project-groups="appSettings.project_groups"
      :active-project-group-id="session.activeProjectGroupId"
      @activate="activateTab"
      @close="requestCloseTab"
      @action="onNewMenuAction"
      @project-action="onProjectAction"
      @reorder="reorderTab"
      @open-plugin="openPlugin"
      @rename="onRenameTab"
      @open-overview="openOverview"
    >
      <template #left>
        <button
          v-if="isBroadcastActive"
          type="button"
          class="tab-bar-icon-btn broadcast-btn"
          :title="t('split.toggleBroadcast')"
          @click="splitPane.toggleBroadcast()"
          @touchend.prevent="splitPane.toggleBroadcast()"
        >
          <Radar :size="16" />
        </button>
      </template>
      <template #right>
        <button
          v-if="activeTabType === 'terminal'"
          type="button"
          class="tab-bar-icon-btn"
          :title="t('app.preview')"
          @click="openPreview"
          @touchend.prevent="openPreview"
        >
          <Monitor :size="16" />
        </button>
        <button
          type="button"
          class="tab-bar-icon-btn"
          :title="t('app.settings')"
          @click="settingsOpen = true"
          @touchend.prevent="settingsOpen = true"
        >
          <Settings :size="16" />
        </button>
        <button
          type="button"
          class="tab-bar-icon-btn"
          :title="t('agent.title')"
          @click="agentOpen = true"
          @touchend.prevent="agentOpen = true"
        >
          <Bot :size="16" />
        </button>
        <button
          v-if="notif.notifications.value.length > 0"
          type="button"
          class="tab-bar-icon-btn notif-btn"
          :title="t('notification.title')"
          @click="notif.togglePanel()"
          @touchend.prevent="notif.togglePanel()"
        >
          <Bell :size="16" />
          <span v-if="notif.unreadCount.value > 0" class="notif-badge">{{
            notif.unreadCount.value > 9 ? '9+' : notif.unreadCount.value
          }}</span>
        </button>
      </template>
    </TabBar>

    <div id="tab-content" @touchend="onTerminalTouch">
      <div
        v-for="tab in filteredTabs"
        :key="tabKey(tab)"
        class="tab-page"
        :class="{
          active: tab.paneId === activePaneId,
          'has-preview': tab.type === 'terminal' && tab.previewVisible,
          ['pos-' + resolvedPosition]: tab.type === 'terminal' && tab.previewVisible,
        }"
      >
        <template v-if="tab.type === 'terminal'">
          <SplitContainer
            :layout="tab.layout"
            :active-pane-id="tab.activePaneId"
            :broadcast-mode="tab.broadcastMode"
            :broadcast-activity="tab.broadcastActivity"
            :allow-close="getAllLeaves(tab.layout).length > 1"
            :restore-context-map="restoreContexts"
            @register="registerTermRef"
            @title-change="onTitleChange"
            @focus="(id: string) => splitPane.focusPane(id)"
            @close="(id: string) => onClosePane(tab.paneId, id)"
            @input="(id: string, data: string) => splitPane.onTerminalInput(id, data)"
            @file-click="onFileClick"
            @preview-link="onPreviewLink"
            @link-activate="onLinkActivate"
            @reorder="
              (src: string, tgt: string, pos: 'left' | 'right' | 'top' | 'bottom') =>
                splitPane.reorderPane(src, tgt, pos)
            "
            @divider-drag-end="onDividerDragEnd(tab)"
            @restore-dismiss="(id: string) => dismissRestoreContext(id)"
            @split="
              (id: string, dir: 'horizontal' | 'vertical') => {
                splitPane.focusPane(id)
                splitPane.splitPane(dir)
              }
            "
          />
          <PreviewPanel
            v-if="tab.paneId === activePaneId"
            :ref="setPreviewPanelRef"
            :visible="tab.previewVisible"
            :pane-id="tab.activePaneId"
            :address="tab.previewAddress"
            :kind="tab.previewKind"
            :web-url="tab.previewUrl"
            :panel-position="resolvedPosition"
            @close="closePreview(tab.paneId)"
            @update:address="
              (v: string) => {
                tab.previewAddress = v
                persist()
              }
            "
            @update:kind="
              (v: 'web' | 'files') => {
                tab.previewKind = v
                persist()
              }
            "
            @update:web-url="
              (v: string) => {
                tab.previewUrl = v
                persist()
              }
            "
          />
        </template>
        <PluginView
          v-else-if="tab.type === 'plugin'"
          :data-plugin-pane-id="tab.paneId"
          :plugin="loadedPlugins.get(tab.pluginId)!"
          :api="getPluginContext(tab.pluginId)"
        />
      </div>
    </div>

    <NotificationPanel :pane-labels="paneLabels" @goto-pane="activateTab" />

    <StatusBar />

    <CommandPalette ref="paletteRef" :commands="paletteCommands" />

    <SettingsPanel :open="settingsOpen" @close="settingsOpen = false" />
    <AgentPanel :open="agentOpen" @close="agentOpen = false" />

    <ConfirmCloseDialog @confirm="onConfirmClose" />

    <ConfirmModal
      :visible="windowCloseConfirmVisible"
      :title="t('confirm.closeWindowTitle')"
      :message="t('confirm.closeWindowMessage')"
      :confirm-text="t('confirm.closeWindowConfirm')"
      :cancel-text="t('confirm.closeWindowCancel')"
      @confirm="onWindowCloseConfirm"
      @cancel="onWindowCloseCancel"
    />

    <CommandBookmarks ref="bookmarksRef" :get-send-fn="getSendFn" :create-tab="newTab" />

    <ServerList ref="serverListRef" @connect="onServerConnect" />

    <MobileKeyboard
      :visible="kbVisible"
      :pane-id="activePaneId ?? ''"
      :get-send-fn="getSendFn"
      @update:visible="(v: boolean) => (kbVisible = v)"
      @bookmarks="bookmarksRef?.open()"
    />

    <KbToggleButton
      v-show="appSettings.show_virtual_keyboard && !kbVisible"
      :visible="kbVisible"
      @toggle="kbVisible = !kbVisible"
    />

    <TabOverview
      :visible="overviewOpen"
      :cards="overviewCards"
      :active-pane-id="activePaneId"
      @close="overviewOpen = false"
      @activate="onOverviewActivate"
      @close-tab="onOverviewCloseTab"
    />
  </div>
</template>

<script setup lang="ts">
import {
  ref,
  reactive,
  shallowReactive,
  computed,
  watch,
  onMounted,
  onBeforeUnmount,
  nextTick,
} from 'vue'
import TabBar from './components/terminal/TabBar.vue'
import type { TabInfo } from './components/terminal/TabBar.vue'
import TerminalPane from './components/terminal/TerminalPane.vue'
import SplitContainer from './components/split/SplitContainer.vue'
import CommandPalette from './components/command/CommandPalette.vue'
import type { Command } from './components/command/CommandPalette.vue'
import MobileKeyboard from './components/keyboard/MobileKeyboard.vue'
import KbToggleButton from './components/keyboard/KbToggleButton.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import AgentPanel from './components/agent/AgentPanel.vue'
import ConfirmCloseDialog from './components/ui/ConfirmCloseDialog.vue'
import ConfirmModal from './components/ui/ConfirmModal.vue'
import PreviewPanel from './components/preview/PreviewPanel.vue'
import CommandBookmarks from './components/command/CommandBookmarks.vue'
import ServerList from './components/ServerList.vue'
import StatusBar from './components/terminal/StatusBar.vue'
import type { Tab, TerminalTab, PluginTab, PaneLayout } from './types/pane'
import { getAllLeaves, findLeaf, findFirstLeaf, ensureSplitRoot } from './types/pane'
// useSettings replaced by useSettingsStore
import {
  getApiBase,
  checkTokenConfigured,
  setAuthToken,
  setDesktopId,
} from './composables/apiBase'
import { isTauri, tauriInvoke } from './composables/useTransport'
import { isTouchDevice, setActivePaneId } from './composables/useTerminal'
import { useI18n } from './composables/useI18n'
import { useKeybindings } from './composables/useKeybindings'
import { useSplitPane } from './composables/useSplitPane'
import { useSyncWebSocket } from './composables/useSyncWebSocket'
import { isWebPreviewInput } from './utils/previewRouting'
import { initMonitorHistory } from './composables/useMonitor'
import NotificationPanel from './components/notification/NotificationPanel.vue'
import { useNotification } from './composables/useNotification'
import { usePluginLoader } from './composables/usePluginLoader'
import PluginView from './components/plugin/PluginView.vue'
import {
  apiCreateTab,
  apiCloseTab,
  apiClosePane,
  apiActivatePane,
  apiListTabs,
  apiListShellProfiles,
  apiGetRestoreState,
  apiUpdateTabMeta,
} from './composables/useTabApi'
import type { ShellProfile, RestoredPane } from './composables/useTabApi'
import { Settings, Bell, Monitor, Plus, X, Star, AppWindow, Radar, Bot } from 'lucide-vue-next'
import TabOverview from './components/overview/TabOverview.vue'
import type { TabCard } from './composables/useTabPreview'
import { useTabPreview, refreshPluginPreview, invalidatePluginPreview } from './composables/useTabPreview'
import { useIsMobile } from './composables/useIsMobile'
// formatCloseTabMessage moved to ConfirmCloseDialog component
import LoginPage from './components/LoginPage.vue'
import SetupPage from './components/SetupPage.vue'
import { storeToRefs } from 'pinia'
import { useSessionStore } from './stores/sessionStore'
import { useUiStore } from './stores/uiStore'
import { useSettingsStore } from './stores/settingsStore'

import type { ProjectGroup } from './composables/useSettings'

// ── Stores ──────────────────────────────────────────────────────
const session = useSessionStore()
const { tabs, activePaneId, filteredTabs, filteredTabList, activeTabType, isBroadcastActive, canBroadcast, paneLabels } =
  storeToRefs(session)

const ui = useUiStore()
const { syncConnected, kbVisible, settingsOpen, authenticated, needsSetup } = storeToRefs(ui)

const settingsStore = useSettingsStore()
const appSettings = settingsStore.settings

const windowCloseConfirmVisible = ref(false)

// ── Relay / mobile remote mode ──────────────────────────────────
// When the app is loaded via a relay URL (e.g. inside the Dinotty TWA
// on a phone), the URL looks like:
//
//   https://relay/?desktop_id=<uuid>#<password>
//
// The password comes via URL fragment (so it isn't sent to the
// server) and is stashed in localStorage on first load. The normal
// frontend then loads, and all WS / HTTP requests go through the
// relay proxy (wsUrlWithToken adds ?desktop_id= to every WS URL).
// The relay forwards them to the desktop through the outbound tunnel.
function parseRelayMode() {
  if (typeof window === 'undefined') return
  const params = new URLSearchParams(window.location.search)
  const desktopId = params.get('desktop_id')
  if (!desktopId) return
  const fragment = window.location.hash.replace(/^#/, '')
  if (fragment) {
    setAuthToken(fragment)
  }
  setDesktopId(desktopId)
}

let linkJustActivated = false

// ── Template refs (purely UI concerns) ─────────────────────────
const paletteRef = ref<InstanceType<typeof CommandPalette>>()
const previewPanelRef = ref<InstanceType<typeof PreviewPanel> | null>(null)

function setPreviewPanelRef(el: any) {
  previewPanelRef.value = el
}
const bookmarksRef = ref<InstanceType<typeof CommandBookmarks>>()
const serverListRef = ref<InstanceType<typeof ServerList>>()
const { t } = useI18n()
const { getBinding, formatBinding } = useKeybindings()
const notif = useNotification()
const { loadedPlugins, loadAll, getPluginContext, pluginList, allCommands } = usePluginLoader()
const { isMobile } = useIsMobile()
const tabPreview = useTabPreview()
const shellProfiles = ref<ShellProfile[]>([])
const defaultShellProfileId = ref<string | null>(null)
const restoreContexts = ref<Record<string, RestoredPane>>({})

function restoreContextFor(paneId: string): RestoredPane | null {
  return restoreContexts.value[paneId] ?? null
}

function dismissRestoreContext(paneId: string) {
  const next = { ...restoreContexts.value }
  delete next[paneId]
  restoreContexts.value = next
}

async function loadRestoreContexts() {
  try {
    const state = await apiGetRestoreState()
    restoreContexts.value = Object.fromEntries(state.panes.map((pane) => [pane.pane_id, pane]))
  } catch (e) {
    console.warn('Failed to load restore context:', e)
  }
}

async function loadShellProfiles() {
  try {
    const data = await apiListShellProfiles()
    shellProfiles.value = data.profiles
    defaultShellProfileId.value = data.default_profile_id ?? null
  } catch (e) {
    console.warn('Failed to load shell profiles:', e)
  }
}

const isLandscape = ref(window.innerWidth > window.innerHeight)

// Mission Control
const overviewOpen = ref(false)
const agentOpen = ref(false)
const overviewCards = ref<TabCard[]>([])
const currentTabIndex = computed(() =>
  filteredTabs.value.findIndex((t) => t.paneId === activePaneId.value) + 1
)
const currentTabTitle = computed(() => {
  const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
  if (!tab) return ''
  if (tab.type === 'terminal') return tab.customTitle ?? findLeaf(tab.layout, tab.activePaneId)?.title ?? 'Terminal'
  return tab.title
})

function openOverview() {
  overviewCards.value = tabPreview.captureAll(tabs.value, termRefs, notif.unreadByPane)
  overviewOpen.value = true
}

function onOverviewActivate(paneId: string) {
  activateTab(paneId)
  overviewOpen.value = false
  nextTick(() => {
    const ref = termRefs[paneId]
    ref?.focus()
  })
}

function onOverviewCloseTab(tabId: string) {
  requestCloseTab(tabId)
}

watch(
  () => tabs.value.length,
  () => {
    if (overviewOpen.value) {
      overviewCards.value = tabPreview.captureAll(tabs.value, termRefs, notif.unreadByPane)
    }
  }
)

// Capture plugin preview when active tab changes to a plugin tab (handles initial load)
watch(
  activePaneId,
  (paneId) => {
    const tab = tabs.value.find((t) => t.paneId === paneId)
    if (tab?.type === 'plugin') {
      nextTick(() => refreshPluginPreview(tab.paneId))
    }
  }
)

const resolvedPosition = computed(() => {
  const pos = appSettings.panel_position ?? 'auto'
  if (pos === 'auto') return isLandscape.value ? 'right' : 'top'
  return pos
})

watch(
  () => appSettings.locale,
  (l) => {
    document.documentElement.lang = l === 'en' ? 'en' : 'zh-CN'
  },
  { immediate: true }
)
watch(
  () => {
    const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
    if (!tab) return 'Terminal'
    if (tab.type === 'terminal') {
      return findLeaf(tab.layout, tab.activePaneId)?.title ?? 'Terminal'
    }
    return tab.title
  },
  (title) => {
    document.title = title || 'Terminal'
  },
  { immediate: true }
)

// Track effective active pane for Tauri WKWebView input guard
function syncActivePaneId() {
  const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
  const paneId = tab?.type === 'terminal' ? tab.activePaneId : null
  setActivePaneId(paneId)
}
// Fire on tab switch (store activePaneId change) and initial load
watch(activePaneId, syncActivePaneId, { immediate: true })
// Fire when tab list changes (add/remove) — not deep, just array reference
watch(() => tabs.value.length, syncActivePaneId)
// Fire when active terminal tab's internal focus changes (sync WS, etc.)
watch(
  () => {
    const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
    return tab?.type === 'terminal' ? tab.activePaneId : null
  },
  (paneId) => setActivePaneId(paneId),
)

const termRefs = shallowReactive<Record<string, InstanceType<typeof TerminalPane>>>({})
const outputListeners = new Set<(paneId: string, data: string) => void>()

const syncWs = useSyncWebSocket({
  termRefs,
  persist,
  focusActive,
  newTab: async () => { await newTab() },
})

const splitPane = useSplitPane({
  tabs,
  activePaneId,
  termRefs,
  genPaneId,
  sendSync: syncWs.sendSync,
  sendLayoutSync: syncWs.sendLayoutSync,
  persist,
})

function registerTermRef(paneId: string, el: InstanceType<typeof TerminalPane> | null) {
  if (el) {
    termRefs[paneId] = el
    el.setOutputListener((data: string) => {
      outputListeners.forEach((cb) => cb(paneId, data))
    })
  }
}

let viewportRefitTimer = 0

function onViewportResize() {
  clearTimeout(viewportRefitTimer)
  viewportRefitTimer = window.setTimeout(() => {
    if (!activePaneId.value) return
    const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
    if (!tab || tab.type !== 'terminal') return
    for (const leaf of getAllLeaves(tab.layout)) {
      termRefs[leaf.paneId]?.fit()
    }
  }, 100)
}

function genPaneId(): string {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0
    return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16)
  })
}

/** Stable key for tab v-for — uses the first leaf paneId which never changes */
function tabKey(tab: Tab): string {
  if (tab.type !== 'terminal') return tab.paneId
  const leaf = findFirstLeaf(tab.layout)
  return leaf ? leaf.paneId : tab.paneId
}

function onDividerDragEnd(tab: Tab) {
  if (tab.type === 'terminal') {
    persist()
    syncWs.sendLayoutSync(tab.paneId, tab.layout, tab.activePaneId)
  }
}

let persistTimer: ReturnType<typeof setTimeout> | null = null
function persistNow() {
  const state = tabs.value.map((t) => {
    if (t.type === 'terminal') {
      return {
        type: t.type,
        paneId: t.paneId,
        layout: t.layout,
        activePaneId: t.activePaneId,
        broadcastMode: t.broadcastMode,
        previewVisible: t.previewVisible,
        previewAddress: t.previewAddress,
        previewUrl: t.previewUrl,
        previewKind: t.previewKind,
        customTitle: t.customTitle,
        shellProfileId: t.shellProfileId,
        shellProfileName: t.shellProfileName,
        groupId: t.groupId ?? null,
        workspaceRoots: t.workspaceRoots ?? [],
        restoreContext: t.restoreContext,
      }
    }
    return {
      type: t.type,
      paneId: t.paneId,
      title: t.title,
      pluginId: t.pluginId,
    }
  })
  const activeIdx = tabs.value.findIndex((t) => t.paneId === activePaneId.value)
  localStorage.setItem(
    'dinotty_tabs',
    JSON.stringify({
      tabs: state,
      activeIdx,
      activeProjectGroupId: session.activeProjectGroupId,
    })
  )
}
function persist() {
  if (persistTimer) clearTimeout(persistTimer)
  persistTimer = setTimeout(persistNow, 200)
}
// Flush pending persist on page unload
window.addEventListener('beforeunload', (e) => {
  if (persistTimer) {
    clearTimeout(persistTimer)
    persistNow()
  }
  if (appSettings.confirm_before_close_tab && tabs.value.some((t) => t.type === 'terminal')) {
    e.preventDefault()
    e.returnValue = ''
  }
})

const DEFAULT_PREVIEW_URL = ''

async function newTab(profileId?: string, groupIdOverride?: string | null) {
  try {
    // Prefer the caller-provided group so nextTick-based triggers (e.g.
    // switching projects) can't have their target rewritten by a later
    // setActiveProjectGroup() call that runs before the await resolves.
    const activeGroupId =
      groupIdOverride !== undefined ? groupIdOverride : session.activeProjectGroupId
    const group = appSettings.project_groups.find((g) => g.id === activeGroupId)
    const result = await apiCreateTab({
      ...(profileId ? { profile_id: profileId } : {}),
      group_id: activeGroupId,
      workspace_roots: group?.workspace_roots ?? [],
    })
    // Dedup: broadcast_sync echoes back to sender — tab_created handler may
    // have already added this tab if the sync message arrived before the
    // REST response.
    if (tabs.value.some((t) => t.type === 'terminal' && t.paneId === result.tab_id)) {
      activePaneId.value = result.tab_id
      persist()
      nextTick(() => focusActive())
      return
    }
    const layout = ensureSplitRoot(result.layout)
    tabs.value.push({
      type: 'terminal',
      paneId: result.tab_id,
      layout,
      activePaneId: result.pane_id,
      broadcastMode: false,
      broadcastActivity: 0,
      previewVisible: false,
      previewAddress: '',
      previewUrl: '',
      previewKind: 'web',
      shellProfileId: result.shell_profile_id,
      shellProfileName: result.shell_profile_name,
      groupId: result.group_id ?? null,
      workspaceRoots: result.workspace_roots ?? [],
    })
    activePaneId.value = result.tab_id
    persist()
    nextTick(() => focusActive())
  } catch (e) {
    console.error('Failed to create tab:', e)
    notif.pushToast({
      type: 'error',
      title: t('palette.newTab') + ' failed',
      body: String((e as Error)?.message ?? e),
    })
  }
}

type NewMenuAction =
  | 'new-tab'
  | 'split-h'
  | 'split-v'
  | 'broadcast'
  | { type: 'new-tab-profile'; profileId: string }

function onNewMenuAction(action: NewMenuAction) {
  if (typeof action !== 'string') {
    if (action.type === 'new-tab-profile') return newTab(action.profileId)
    return
  }
  switch (action) {
    case 'new-tab':
      return newTab()
    case 'split-h':
      return splitPane.splitPane('horizontal')
    case 'split-v':
      return splitPane.splitPane('vertical')
    case 'broadcast':
      return splitPane.toggleBroadcast()
  }
}

type ProjectAction =
  | { type: 'select'; groupId: string | null }
  | { type: 'create' }
  | { type: 'move-active'; groupId: string | null }
  | { type: 'rename'; groupId: string }
  | { type: 'move-to'; groupId: string }
  | { type: 'delete'; groupId: string }

async function onProjectAction(action: ProjectAction) {
  if (action.type === 'select') {
    session.setActiveProjectGroup(action.groupId)
    const currentVisible = filteredTabs.value.some((t) => t.paneId === activePaneId.value)
    if (!currentVisible) {
      const first = filteredTabs.value[0]
      if (first) {
        activePaneId.value = first.paneId
      } else if (action.groupId) {
        // Switching to an empty group — create a tab in it.
        // Capture the target now so a subsequent project switch can't
        // reroute this newTab into a different project.
        const targetGroup = action.groupId
        nextTick(() => newTab(undefined, targetGroup))
      }
    }
    return
  }
  if (action.type === 'create') {
    const name = window.prompt(t('project.promptCreate'))?.trim()
    if (!name) return
    const now = Date.now()
    const group: ProjectGroup = {
      id: `project-${now}`,
      name,
      color: null,
      workspace_roots: [],
      created_at: now,
      updated_at: now,
      sort_order: appSettings.project_groups.length,
      archived: false,
    }
    appSettings.project_groups.push(group)
    appSettings.default_project_group_id ??= group.id
    // Auto-assign all ungrouped terminal tabs to the new group — both locally
    // and on the backend, otherwise the move is lost on the next reload.
    const autoAssigned: Array<{ paneId: string; workspaceRoots: string[] }> = []
    for (const tab of tabs.value) {
      if (tab.type === 'terminal' && !tab.groupId) {
        session.moveTabToProjectGroup(tab.paneId, group.id)
        autoAssigned.push({
          paneId: tab.paneId,
          workspaceRoots: tab.workspaceRoots ?? [],
        })
      }
    }
    session.setActiveProjectGroup(group.id)

    // Persist the project itself first; failure here rolls back local state.
    try {
      await settingsStore.save()
    } catch (e) {
      console.error('Failed to save project:', e)
      notif.pushToast({
        type: 'error',
        title: t('project.error.createFailed'),
        body: String((e as Error)?.message ?? e),
      })
      const idx = appSettings.project_groups.findIndex((g) => g.id === group.id)
      if (idx !== -1) appSettings.project_groups.splice(idx, 1)
      if (appSettings.default_project_group_id === group.id) {
        appSettings.default_project_group_id = null
      }
      for (const { paneId } of autoAssigned) {
        session.moveTabToProjectGroup(paneId, null)
      }
      session.setActiveProjectGroup(null)
      return
    }

    // Now persist each auto-assigned tab meta. If any one fails, leave the
    // local group-id as-is — the next reload will re-sync from the server
    // (acceptable since the project itself was saved).
    let allMetaOk = true
    for (const { paneId, workspaceRoots } of autoAssigned) {
      try {
        await apiUpdateTabMeta(paneId, {
          group_id: group.id,
          workspace_roots: workspaceRoots,
        })
      } catch (e) {
        console.error('Failed to sync tab meta:', e)
        allMetaOk = false
      }
    }
    if (!allMetaOk) {
      notif.pushToast({ type: 'warning', title: t('project.error.syncPartial') })
    }
    persist()
    return
  }
  if (action.type === 'move-active') {
    if (!activePaneId.value) return
    // No-op if the active tab is already in the requested project.
    const cur = tabs.value.find((t) => t.paneId === activePaneId.value) as TerminalTab | undefined
    if (!cur) return
    if ((cur.groupId ?? null) === action.groupId) return
    const tabId = cur.paneId
    const workspaceRoots = cur.workspaceRoots ?? []
    const originalGroup = cur.groupId ?? null
    session.moveTabToProjectGroup(tabId, action.groupId)
    try {
      await apiUpdateTabMeta(cur.paneId, {
        group_id: action.groupId,
        workspace_roots: workspaceRoots,
      })
    } catch (e) {
      console.error('Failed to sync tab meta:', e)
      notif.pushToast({
        type: 'error',
        title: t('project.error.moveFailed'),
        body: String((e as Error)?.message ?? e),
      })
      // Roll back local change so UI doesn't lie.
      session.moveTabToProjectGroup(tabId, originalGroup)
      return
    }
    persist()
    return
  }
  if (action.type === 'rename') {
    const group = appSettings.project_groups.find((g) => g.id === action.groupId)
    if (!group) return
    const name = window.prompt(t('project.promptRename'), group.name)?.trim()
    if (!name || name === group.name) return
    const oldName = group.name
    group.name = name
    group.updated_at = Date.now()
    try {
      await settingsStore.save()
    } catch (e) {
      console.error('Failed to rename project:', e)
      notif.pushToast({
        type: 'error',
        title: t('project.error.renameFailed'),
        body: String((e as Error)?.message ?? e),
      })
      group.name = oldName
      return
    }
    return
  }
  if (action.type === 'move-to') {
    if (!activePaneId.value) return
    const cur = tabs.value.find((t) => t.paneId === activePaneId.value) as TerminalTab | undefined
    if (!cur) return
    if ((cur.groupId ?? null) === action.groupId) return
    const tabId = cur.paneId
    const workspaceRoots = cur.workspaceRoots ?? []
    const originalGroup = cur.groupId ?? null
    session.moveTabToProjectGroup(tabId, action.groupId)
    try {
      await apiUpdateTabMeta(cur.paneId, {
        group_id: action.groupId,
        workspace_roots: workspaceRoots,
      })
    } catch (e) {
      console.error('Failed to sync tab meta:', e)
      notif.pushToast({
        type: 'error',
        title: t('project.error.moveFailed'),
        body: String((e as Error)?.message ?? e),
      })
      session.moveTabToProjectGroup(tabId, originalGroup)
      return
    }
    session.setActiveProjectGroup(action.groupId)
    persist()
  }
  if (action.type === 'delete') {
    const group = appSettings.project_groups.find((g) => g.id === action.groupId)
    if (!group) return
    const tabCount = tabs.value.filter(
      (t) => t.type === 'terminal' && t.groupId === action.groupId
    ).length
    const msg = tabCount > 0
      ? t('project.confirmDeleteWithTabs', { name: group.name, n: tabCount })
      : t('project.confirmDelete', { name: group.name })
    if (!window.confirm(msg)) return

    const idx = appSettings.project_groups.findIndex((g) => g.id === action.groupId)
    if (idx === -1) return

    // Move all tabs in this group to "No Project" first — both locally and
    // on the backend, in order.
    const affected: Array<{ tabId: string; workspaceRoots: string[] }> = []
    for (const tab of tabs.value) {
      if (tab.type === 'terminal' && tab.groupId === action.groupId) {
        affected.push({ tabId: tab.paneId, workspaceRoots: tab.workspaceRoots ?? [] })
        session.moveTabToProjectGroup(tab.paneId, null)
      }
    }

    appSettings.project_groups.splice(idx, 1)
    if (appSettings.default_project_group_id === action.groupId) {
      appSettings.default_project_group_id = null
    }
    const wasActive = session.activeProjectGroupId === action.groupId
    if (wasActive) {
      session.setActiveProjectGroup(null)
    }

    try {
      await settingsStore.save()
    } catch (e) {
      console.error('Failed to save after delete:', e)
      notif.pushToast({
        type: 'error',
        title: t('project.error.deleteFailed'),
        body: String((e as Error)?.message ?? e),
      })
      // Roll back EVERYTHING we changed locally
      appSettings.project_groups.splice(idx, 0, group)
      if (group.id) appSettings.default_project_group_id = group.id
      if (wasActive) session.setActiveProjectGroup(action.groupId)
      for (const { tabId } of affected) {
        session.moveTabToProjectGroup(tabId, action.groupId)
      }
      return
    }

    let allMetaOk = true
    for (const { tabId, workspaceRoots } of affected) {
      try {
        await apiUpdateTabMeta(tabId, {
          group_id: null,
          workspace_roots: workspaceRoots,
        })
      } catch (e) {
        console.error('Failed to sync tab meta after delete:', e)
        allMetaOk = false
      }
    }
    if (!allMetaOk) {
      notif.pushToast({ type: 'warning', title: t('project.error.syncPartial') })
    }
    persist()
  }
}

async function activateTab(tabId: string) {
  activePaneId.value = tabId
  const tab = tabs.value.find((t) => t.paneId === tabId)
  if (tab?.type === 'terminal') {
    notif.clearPaneUnread(tab.activePaneId)
    try {
      await apiActivatePane(tab.paneId, tab.activePaneId)
    } catch (e) {
      console.error('Failed to activate pane:', e)
    }
  }
  // Capture plugin preview when switching to a plugin tab
  if (tab?.type === 'plugin') {
    nextTick(() => refreshPluginPreview(tab.paneId))
  }
  persist()
  nextTick(() => focusActive())
}

function reorderTab(fromId: string, toId: string) {
  session.reorderTab(fromId, toId)
  persist()
}

function onRenameTab(paneId: string, title: string) {
  session.renameTab(paneId, title)
  persist()
}

async function onClosePane(tabId: string, paneId: string) {
  const tab = tabs.value.find((t) => t.paneId === tabId)
  if (!tab) return

  // Bypass 1: non-terminal tab
  if (tab.type !== 'terminal') {
    const closed = await splitPane.closePane(paneId)
    if (!closed) await closeTab(tabId)
    return
  }

  // Bypass 2: user disabled confirmation
  if (appSettings.confirm_before_close_tab === false) {
    const closed = await splitPane.closePane(paneId)
    if (!closed) await closeTab(tabId)
    return
  }

  // Show confirmation (handles both pane and tab close)
  ui.requestClosePane(tabId, paneId)
}

async function requestCloseTab(tabId: string) {
  const tab = tabs.value.find((t) => t.paneId === tabId)
  if (!tab) return

  // Bypass 1: non-terminal tabs (plugins) — close immediately, no prompt
  if (tab.type !== 'terminal') {
    await closeTab(tabId)
    return
  }

  // Bypass 2: user disabled confirmation in settings
  if (appSettings.confirm_before_close_tab === false) {
    await closeTab(tabId)
    return
  }

  // Otherwise: show confirmation
  ui.requestCloseTab(tabId)
}

async function onConfirmClose(tabId: string, paneId: string | null) {
  if (paneId) {
    // Pane close: try close pane first, fall back to tab close if last
    const closed = await splitPane.closePane(paneId)
    if (!closed && tabId) {
      await closeTab(tabId)
    }
  } else if (tabId) {
    // Tab close (no pane specified)
    await closeTab(tabId)
  }
  ui.cancelClose()
}

async function closeTab(tabId: string) {
  const tab = tabs.value.find((t) => t.paneId === tabId)
  if (!tab) return

  // Invalidate plugin preview cache when closing a plugin tab
  if (tab.type === 'plugin') {
    invalidatePluginPreview(tab.paneId)
  }

  if (tab.type === 'terminal') {
    // Clean up local term refs
    for (const leaf of getAllLeaves(tab.layout)) {
      delete termRefs[leaf.paneId]
    }

    try {
      await apiCloseTab(tabId)
    } catch (e) {
      console.error('Failed to close tab:', e)
      return
    }
  }

  // Remove tab from local array
  const idx = tabs.value.findIndex((t) => t.paneId === tabId)
  if (idx === -1) return

  tabs.value.splice(idx, 1)

  // If this was the last tab, create a new one
  if (tabs.value.length === 0) {
    await newTab()
    return
  }

  if (activePaneId.value === tabId) {
    // Prefer a tab visible under the current project filter
    const visible = filteredTabs.value
    if (visible.length > 0) {
      const visibleIdx = visible.findIndex((t) => t.paneId === tabId)
      const newVisibleIdx = Math.min(visibleIdx >= 0 ? visibleIdx : 0, visible.length - 1)
      activePaneId.value = visible[newVisibleIdx].paneId
    } else if (tabs.value.length > 0) {
      // No visible tabs — reset filter so the user sees something
      session.setActiveProjectGroup(null)
      const newIdx = Math.min(idx, tabs.value.length - 1)
      activePaneId.value = tabs.value[newIdx].paneId
    }
  }

  persist()
  nextTick(() => focusActive())
}

function focusActive() {
  if (!activePaneId.value) return
  const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
  if (!tab) return
  if (tab.type === 'terminal' && tab.layout) {
    const paneId = tab.activePaneId
    if (!(isTouchDevice() && kbVisible.value)) {
      // Blur all other panes first to prevent duplicate input in Tauri WKWebView
      for (const leaf of getAllLeaves(tab.layout)) {
        if (leaf.paneId !== paneId) {
          termRefs[leaf.paneId]?.blur()
        }
      }
      termRefs[paneId]?.focus()
    }
    termRefs[paneId]?.fit()
  }
}

function onTitleChange(paneId: string, title: string) {
  // Find terminal tab containing this leaf pane
  const tab = tabs.value.find((t) => {
    if (t.type !== 'terminal') return false
    return !!findLeaf(t.layout, paneId)
  }) as TerminalTab | undefined
  if (tab) {
    const leaf = findLeaf(tab.layout, paneId)
    if (leaf) {
      leaf.title = title || 'Terminal'
      persist()
    }
  }
}

function onPreviewLink(leafPaneId: string, url: string) {
  const tab = tabs.value.find((t) => {
    if (t.type !== 'terminal') return false
    return !!findLeaf(t.layout, leafPaneId)
  }) as TerminalTab | undefined
  if (!tab) return
  tab.previewKind = 'web'
  tab.previewUrl = url
  tab.previewAddress = url
  tab.previewVisible = true
  persist()
}

function closePreview(tabId: string) {
  const tab = tabs.value.find((t) => t.paneId === tabId)
  if (tab && tab.type === 'terminal') {
    tab.previewVisible = false
    persist()
  }
}

function openPreview() {
  const tabId = activePaneId.value
  if (!tabId) return
  const tab = tabs.value.find((t) => t.paneId === tabId)
  if (!tab || tab.type !== 'terminal') return
  if (!tab.previewAddress.trim()) {
    tab.previewKind = 'files'
  }
  tab.previewVisible = true
  persist()
  nextTick(() => {
    if (tab.previewKind !== 'files') return
    const raw = tab.previewAddress.trim()
    if (raw && !isWebPreviewInput(raw)) {
      previewPanelRef.value?.openFromPath(raw)
    }
  })
}

function onFileClick(path: string) {
  const tabId = activePaneId.value
  if (!tabId) return
  const tab = tabs.value.find((t) => t.paneId === tabId)
  if (!tab || tab.type !== 'terminal') return
  tab.previewKind = 'files'
  tab.previewAddress = path
  tab.previewVisible = true
  persist()
  nextTick(() => previewPanelRef.value?.openFromPath(path))
}

function getSendFn(): ((data: string) => void) | null {
  if (!activePaneId.value) return null
  const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
  if (!tab || tab.type !== 'terminal') return null
  const paneId = tab.activePaneId
  if (!termRefs[paneId]) return null
  return (data: string) => termRefs[paneId]?.sendData(data)
}

async function onLoginSuccess() {
  ui.setAuthenticated(true)
  await getApiBase()
  await settingsStore.load()
  // Restore the last-active project so the user lands in the right context
  // instead of always starting in "No Project". Prefer the localStorage value
  // (last user choice); fall back to the first project ever created, then null.
  let restoredProjectId: string | null = null
  try {
    const raw = localStorage.getItem('dinotty_tabs')
    if (raw) {
      const parsed = JSON.parse(raw)
      if (typeof parsed?.activeProjectGroupId === 'string') {
        restoredProjectId = parsed.activeProjectGroupId
      }
    }
  } catch {
    /* ignore corrupt localStorage */
  }
  if (!restoredProjectId && appSettings.default_project_group_id) {
    restoredProjectId = appSettings.default_project_group_id
  }
  if (restoredProjectId) {
    const exists = appSettings.project_groups.some((g) => g.id === restoredProjectId)
    if (exists) {
      session.setActiveProjectGroup(restoredProjectId)
    }
  }
  await loadShellProfiles()
  await loadRestoreContexts()
  void loadAll()
  void syncWs.connectSyncWS()
  initMonitorHistory()
}

function shellEscapePath(path: string): string {
  return /[\s'"\\()&;|<>$!`{}[\]#?*~]/.test(path) ? `'${path.replace(/'/g, "'\\''")}'` : path
}

function onTerminalInsertPath(e: Event) {
  const path = (e as CustomEvent<{ path: string }>).detail?.path
  if (!path) return
  const send = getSendFn()
  if (send) send(shellEscapePath(path) + ' ')
}

function onTerminalInsertText(e: Event) {
  const text = (e as CustomEvent<{ text: string }>).detail?.text
  if (!text) return
  const send = getSendFn()
  if (send) send(text)
}

function onLinkActivate() {
  linkJustActivated = true
}

function onTerminalTouch(e: TouchEvent) {
  if (!isTouchDevice()) return
  const target = e.target as HTMLElement
  if (target.closest('.terminal-pane-container')) {
    // Don't show keyboard when tapping a link (file path or URL)
    if (linkJustActivated) {
      linkJustActivated = false
      return
    }
    const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
    const paneId = tab?.type === 'terminal' ? tab.activePaneId : null
    const term = paneId ? termRefs[paneId]?.getTerminal() : null
    if (term && term.touchMoved) {
      term.touchMoved = false
      if (kbVisible.value) kbVisible.value = false
      return
    }
    kbVisible.value = true
  }
}

function onServerConnect(host: string, port: number) {
  const proto = location.protocol
  window.location.href = `${proto}//${host}:${port}/`
}

function openPlugin(pluginId: string) {
  try {
    const paneId = `plugin:${pluginId}`
    const existing = tabs.value.find((t) => t.paneId === paneId)
    if (existing) {
      activateTab(paneId)
      return
    }

    const plugin = loadedPlugins.get(pluginId)
    if (!plugin || plugin.state !== 'active') {
      const msg =
        plugin?.state === 'error'
          ? `Plugin "${pluginId}" failed to load: ${plugin.error ?? 'unknown error'}`
          : `Plugin "${pluginId}" is not loaded.`
      console.warn('[openPlugin]', msg)
      window.__dinotty_ui_notify?.(msg, 'error')
      return
    }

    const newTab = {
      type: 'plugin' as const,
      paneId,
      title: plugin.manifest.name,
      pluginId,
    }
    tabs.value.push(newTab)
    activePaneId.value = paneId
    syncWs.sendSync({ type: 'activate_tab', pane_id: paneId })
    persist()
    nextTick(() => focusActive())
  } catch (err) {
    console.error('[openPlugin] error:', err)
  }
}

// Window globals for plugin context
window.__dinotty_terminal_api = {
  send(paneId: string, data: string) {
    termRefs[paneId]?.sendData(data)
  },
  activePaneId() {
    const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
    return tab?.type === 'terminal' ? tab.activePaneId : activePaneId.value
  },
  listPanes() {
    const result: { id: string; title: string; active: boolean }[] = []
    for (const t of tabs.value) {
      if (t.type !== 'terminal') continue
      for (const leaf of getAllLeaves(t.layout)) {
        result.push({
          id: leaf.paneId,
          title: leaf.title,
          active: t.paneId === activePaneId.value && leaf.paneId === t.activePaneId,
        })
      }
    }
    return result
  },
  onOutput(callback: (paneId: string, data: string) => void) {
    outputListeners.add(callback)
    return {
      dispose() {
        outputListeners.delete(callback)
      },
    }
  },
  async createTab(command?: string) {
    newTab()
    const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
    return tab?.type === 'terminal' ? tab.activePaneId : ''
  },
}
window.__dinotty_ui_notify = (message: string, level?: 'info' | 'warn' | 'error') => {
  // Use notification system or console
  if (level === 'error') console.error('[plugin]', message)
  else console.log('[plugin]', message)
}
window.__dinotty_ui_confirm = async (message: string) => window.confirm(message)
window.__dinotty_open_plugin = openPlugin

const paletteCommands = computed<Command[]>(() => {
  const base: Command[] = [
    {
      icon: '＋',
      title: t('palette.newTab'),
      subtitle: t('palette.newTabDesc'),
      kbd: formatBinding(getBinding('newTab')),
      action: () => newTab(),
    },
    {
      icon: '✕',
      title: t('palette.closeTab'),
      subtitle: t('palette.closeTabDesc'),
      kbd: formatBinding(getBinding('closeTab')),
      action: async () => {
        if (activePaneId.value) {
          const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
          if (tab?.type === 'terminal' && getAllLeaves(tab.layout).length > 1) {
            await onClosePane(tab.paneId, tab.activePaneId)
          } else {
            await requestCloseTab(activePaneId.value)
          }
        }
      },
    },
    {
      icon: '⊞',
      title: t('palette.splitHorizontal'),
      subtitle: t('palette.splitHorizontalDesc'),
      kbd: formatBinding(getBinding('splitHorizontal')),
      action: () => splitPane.splitPane('horizontal'),
    },
    {
      icon: '⊟',
      title: t('palette.splitVertical'),
      subtitle: t('palette.splitVerticalDesc'),
      kbd: formatBinding(getBinding('splitVertical')),
      action: () => splitPane.splitPane('vertical'),
    },
    {
      icon: '★',
      title: t('palette.bookmarks'),
      subtitle: t('palette.bookmarksDesc'),
      kbd: formatBinding(getBinding('openBookmarks')),
      action: () => bookmarksRef.value?.open(),
    },
    {
      icon: '⊡',
      title: t('palette.openPreview'),
      subtitle: t('palette.openPreviewDesc'),
      action: () => openPreview(),
    },
  ]

  // Plugin-registered commands
  for (const cmd of allCommands.value) {
    const plugin = loadedPlugins.get(cmd.pluginId)
    // Look up title from manifest commands list
    const cmdDef = plugin?.manifest.commands?.find((c) => c.id === cmd.id)
    base.push({
      icon: '◈',
      title: cmdDef?.title || cmd.id,
      subtitle: plugin?.manifest.name,
      action: cmd.handler,
    })
  }

  // Plugin open commands (skip if plugin already registered its own commands)
  const pluginsWithCommands = new Set(allCommands.value.map((c) => c.pluginId))
  for (const p of pluginList.value) {
    if (p.state === 'active' && !pluginsWithCommands.has(p.id)) {
      base.push({
        icon: '◈',
        title: t('palette.openPlugin', { name: p.name }),
        subtitle: t('palette.openPluginDesc'),
        action: () => openPlugin(p.id),
      })
    }
  }

  return base
})

function onGlobalKeydown(e: KeyboardEvent) {
  const cmd = e.metaKey || e.ctrlKey
  if (!cmd) return

  const keyActions: Record<string, () => void> = {
    togglePalette: () => paletteRef.value?.toggle(),
    openBookmarks: () => bookmarksRef.value?.open(),
    newTab: () => newTab(),
    closeTab: async () => {
      if (!activePaneId.value) return
      const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
      if (tab?.type === 'terminal' && getAllLeaves(tab.layout).length > 1) {
        // Multi-pane: route through confirmation gate (consistent with X button)
        await onClosePane(tab.paneId, tab.activePaneId)
      } else {
        await requestCloseTab(activePaneId.value)
      }
    },
    splitHorizontal: () => splitPane.splitPane('horizontal'),
    splitVertical: () => splitPane.splitPane('vertical'),
    toggleBroadcast: () => splitPane.toggleBroadcast(),
    toggleZoom: () => splitPane.toggleZoom(),
    equalizePanes: () => splitPane.equalizePanes(),
    focusNextPane: () => splitPane.focusNext(),
    focusPrevPane: () => splitPane.focusPrev(),
    searchTerminal: () => {
      if (!activePaneId.value) return
      const tab = tabs.value.find((t) => t.paneId === activePaneId.value)
      if (!tab || tab.type !== 'terminal') return
      termRefs[tab.activePaneId]?.toggleSearch()
    },
    missionControl: () => openOverview(),
  }

  for (const [id, action] of Object.entries(keyActions)) {
    const binding = getBinding(id)
    const eventKey = binding.key.length === 1 ? e.key.toLowerCase() : e.key
    if (eventKey === binding.key.toLowerCase() && e.shiftKey === binding.shift) {
      e.preventDefault()
      action()
      return
    }
  }

  // Cmd+Option+Arrow: focus neighbor pane (spatial navigation)
  if (e.altKey && !e.shiftKey) {
    const dirMap: Record<string, 'left' | 'right' | 'up' | 'down'> = {
      ArrowLeft: 'left',
      ArrowRight: 'right',
      ArrowUp: 'up',
      ArrowDown: 'down',
    }
    if (dirMap[e.key]) {
      e.preventDefault()
      splitPane.focusNeighbor(dirMap[e.key])
      return
    }
  }

  // Cmd+Option+Shift+Arrow: keyboard resize
  if (e.altKey && e.shiftKey) {
    const dirMap: Record<string, 'left' | 'right' | 'up' | 'down'> = {
      ArrowLeft: 'left',
      ArrowRight: 'right',
      ArrowUp: 'up',
      ArrowDown: 'down',
    }
    if (dirMap[e.key]) {
      e.preventDefault()
      splitPane.keyboardResize(dirMap[e.key])
      return
    }
  }

  if (!e.shiftKey && e.key >= '1' && e.key <= '9') {
    const idx = parseInt(e.key) - 1
    if (idx < filteredTabs.value.length) {
      e.preventDefault()
      activateTab(filteredTabs.value[idx].paneId)
    }
  }
}

function onOrientationChange() {
  isLandscape.value = window.innerWidth > window.innerHeight
}

const _focusHandler = () => {
  nextTick(() => focusActive())
}

// Tauri window close confirmation
let unlistenWindowClose: (() => void) | undefined
function setupTauriWindowClose() {
  if (!isTauri()) return
  const listen = (window as any).__TAURI__?.event?.listen
  if (!listen) return
  listen('window-close-requested', () => {
    if (appSettings.confirm_before_close_tab && tabs.value.some((t) => t.type === 'terminal')) {
      windowCloseConfirmVisible.value = true
    } else {
      tauriInvoke('close_window')
    }
  }).then((fn: () => void) => {
    unlistenWindowClose = fn
  })
}
function onWindowCloseConfirm() {
  windowCloseConfirmVisible.value = false
  if (persistTimer) {
    clearTimeout(persistTimer)
    persistNow()
  }
  tauriInvoke('close_window')
}
function onWindowCloseCancel() {
  windowCloseConfirmVisible.value = false
}

onMounted(async () => {
  parseRelayMode()
  setupTauriWindowClose()
  document.addEventListener('keydown', onGlobalKeydown)
  window.addEventListener('focus', _focusHandler)
  window.addEventListener('resize', onOrientationChange)
  window.addEventListener('terminal-insert-path', onTerminalInsertPath)
  window.addEventListener('terminal-insert-text', onTerminalInsertText)
  if (window.visualViewport) {
    window.visualViewport.addEventListener('resize', onViewportResize)
  }
  if (authenticated.value) {
    await getApiBase()
    await loadShellProfiles()
    await loadRestoreContexts()
    void syncWs.connectSyncWS()
    initMonitorHistory()
    void loadAll()
    // Fallback: if sync WS hasn't delivered tabs within 3s, load via REST
    setTimeout(async () => {
      if (tabs.value.length === 0 && !syncWs.isConnected()) {
        try {
          const data = await apiListTabs()
          for (const tab of data.tabs) {
            if (tabs.value.some((t) => t.paneId === tab.tab_id)) continue
            const layout = tab.layout
              ? ensureSplitRoot(tab.layout)
              : ensureSplitRoot({
                  type: 'leaf',
                  paneId: tab.pane_id,
                  title: 'Terminal',
                  ratio: 1,
                  zoomed: false,
                })
            tabs.value.push({
              type: 'terminal',
              paneId: tab.tab_id,
              layout,
              activePaneId: tab.active_pane_id ?? tab.pane_id,
              broadcastMode: false,
              broadcastActivity: 0,
              previewVisible: false,
              previewAddress: '',
              previewUrl: '',
              previewKind: 'web',
              shellProfileId: tab.shell_profile_id,
              shellProfileName: tab.shell_profile_name,
              groupId: tab.group_id ?? null,
              workspaceRoots: tab.workspace_roots ?? [],
            })
          }
          if (data.active_pane_id) {
            const targetTab = tabs.value.find((t) => {
              if (t.type !== 'terminal') return false
              return !!findLeaf(t.layout, data.active_pane_id!)
            }) as TerminalTab | undefined
            if (targetTab) {
              activePaneId.value = targetTab.paneId
            }
          }
          if (tabs.value.length > 0 && !activePaneId.value) {
            activePaneId.value = tabs.value[0].paneId
          }
          persist()
          nextTick(() => focusActive())
        } catch (e) {
          console.warn('[sync] REST fallback failed:', e)
        }
      }
    }, 3000)
  } else {
    // No local token — check if server has one configured
    await getApiBase()
    const configured = await checkTokenConfigured()
    if (!configured) {
      // First-time setup: show setup page
      needsSetup.value = true
    }
  }
})

onBeforeUnmount(() => {
  unlistenWindowClose?.()
  document.removeEventListener('keydown', onGlobalKeydown)
  window.removeEventListener('focus', _focusHandler)
  window.removeEventListener('resize', onOrientationChange)
  window.removeEventListener('terminal-insert-path', onTerminalInsertPath)
  window.removeEventListener('terminal-insert-text', onTerminalInsertText)
  if (window.visualViewport) {
    window.visualViewport.removeEventListener('resize', onViewportResize)
  }
  syncWs.closeWs()
})
</script>

<style>
#app-root {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: calc(100% - var(--mkb-height, 0px));
}
.tab-page.active.has-preview {
  display: flex;
}
.tab-page.active.has-preview.pos-right,
.tab-page.active.has-preview.pos-left {
  flex-direction: row;
}
.tab-page.active.has-preview.pos-top,
.tab-page.active.has-preview.pos-bottom {
  flex-direction: column;
}
.tab-page.active.has-preview > .terminal-pane-container,
.tab-page.active.has-preview > .split-container,
.tab-page.active.has-preview > .split-leaf {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}
.tab-page.active.has-preview > .preview-panel {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}
.tab-page.active.has-preview.pos-left > .terminal-pane-container,
.tab-page.active.has-preview.pos-left > .split-container,
.tab-page.active.has-preview.pos-left > .split-leaf,
.tab-page.active.has-preview.pos-top > .terminal-pane-container,
.tab-page.active.has-preview.pos-top > .split-container,
.tab-page.active.has-preview.pos-top > .split-leaf {
  order: 1;
}
.tab-page.active.has-preview.pos-left > .preview-panel,
.tab-page.active.has-preview.pos-top > .preview-panel {
  order: 0;
}
.tab-page.active.has-preview.pos-top > .terminal-pane-container,
.tab-page.active.has-preview.pos-top > .split-container,
.tab-page.active.has-preview.pos-top > .split-leaf,
.tab-page.active.has-preview.pos-bottom > .terminal-pane-container,
.tab-page.active.has-preview.pos-bottom > .split-container,
.tab-page.active.has-preview.pos-bottom > .split-leaf {
  flex: 2;
}
.tab-page.active.has-preview.pos-top > .preview-panel,
.tab-page.active.has-preview.pos-bottom > .preview-panel {
  flex: 1;
}
.broadcast-btn {
  position: relative;
  color: #ef4444;
  animation: broadcast-pulse 2s ease-in-out infinite;
}
@keyframes broadcast-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.5;
  }
}
.notif-btn {
  position: relative;
}
.notif-badge {
  position: absolute;
  top: 2px;
  right: 2px;
  min-width: 14px;
  height: 14px;
  border-radius: 7px;
  background: var(--color-red, #ef4444);
  color: #fff;
  font-size: 9px;
  font-weight: 700;
  line-height: 14px;
  text-align: center;
  padding: 0 3px;
  pointer-events: none;
}
</style>
