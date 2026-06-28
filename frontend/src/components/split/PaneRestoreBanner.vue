<template>
  <div class="pane-restore-banner" :class="{ expanded }">
    <button type="button" class="prb-summary" @click="expanded = !expanded" :title="expanded ? 'Click to collapse' : 'Click to expand'">
      <span class="prb-title">Restored context</span>
      <span class="prb-sep">·</span>
      <span class="prb-line">cwd: {{ restoreContext.cwd ?? 'unknown' }}</span>
      <span class="prb-sep">·</span>
      <span class="prb-line">shell: {{ restoreContext.shell_profile_name ?? restoreContext.shell_profile_id ?? 'default' }}</span>
      <span v-if="recentText" class="prb-sep">·</span>
      <span v-if="recentText" class="prb-line prb-recent">recent: {{ recentText }}</span>
      <button type="button" class="prb-dismiss" @click.stop="emit('dismiss')" title="Dismiss">✕</button>
    </button>
    <pre v-if="expanded && tailText" class="prb-tail">{{ tailText }}</pre>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import type { RestoredPane } from '../../composables/useTabApi'

const props = defineProps<{
  restoreContext: RestoredPane
}>()

const emit = defineEmits<{
  dismiss: []
}>()

const expanded = ref(false)

const recentText = computed(() => {
  const cmds = props.restoreContext.recent_commands ?? []
  if (cmds.length === 0) return ''
  return cmds.slice(-3).join(' · ')
})

const tailText = computed(() => {
  const tail = props.restoreContext.output_tail ?? []
  if (tail.length === 0) return ''
  return tail.slice(-6).join('\n')
})
</script>

<style scoped>
.pane-restore-banner {
  flex-shrink: 0;
  background: var(--bg-secondary, #1e1e1e);
  border-bottom: 1px solid var(--border-color, #333);
  font-size: 11px;
  color: var(--text-secondary, #aaa);
}

.prb-summary {
  display: flex;
  align-items: center;
  width: 100%;
  height: 28px;
  padding: 0 10px;
  background: none;
  border: none;
  color: inherit;
  font: inherit;
  font-size: 11px;
  text-align: left;
  cursor: pointer;
  overflow: hidden;
  white-space: nowrap;
  gap: 6px;
}

.prb-summary:hover {
  background: var(--bg-hover, #252525);
}

.prb-title {
  color: var(--text-primary, #fff);
  font-weight: 600;
  flex-shrink: 0;
}

.prb-sep {
  color: var(--text-muted, #666);
  flex-shrink: 0;
}

.prb-line {
  overflow: hidden;
  text-overflow: ellipsis;
}

.prb-recent {
  color: var(--text-muted, #888);
  font-style: italic;
}

.prb-dismiss {
  flex-shrink: 0;
  margin-left: auto;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: var(--text-muted, #888);
  font-size: 12px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}

.prb-dismiss:hover {
  background: var(--bg-active, #444);
  color: var(--text-primary, #fff);
}

.prb-tail {
  margin: 0;
  padding: 8px 10px;
  max-height: 140px;
  overflow: auto;
  background: var(--bg-primary, #0f0f0f);
  color: var(--text-secondary, #aaa);
  font-family: var(--font-mono, monospace);
  font-size: 11px;
  line-height: 1.4;
  white-space: pre-wrap;
  word-break: break-all;
  border-top: 1px solid var(--border-color, #333);
}
</style>
