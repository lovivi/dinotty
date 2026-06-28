<template>
  <div
    v-if="visible"
    class="inline-autocomplete"
    :style="{ left: cursorX + 'px', top: cursorY + 'px' }"
    @mousedown.prevent
  >
    <span class="autocomplete-suggestion">{{ completion }}</span>
    <kbd class="autocomplete-hint">Tab</kbd>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  suggestion: string
  typedText: string
  cursorX: number
  cursorY: number
  visible: boolean
}>()

const completion = computed(() => {
  if (!props.suggestion || !props.typedText) return ''
  if (props.suggestion.startsWith(props.typedText)) {
    return props.suggestion.substring(props.typedText.length)
  }
  return ''
})
</script>

<style scoped>
.inline-autocomplete {
  position: absolute;
  z-index: 100;
  pointer-events: none;
  display: flex;
  align-items: center;
  font-family: var(--font-mono, monospace);
  line-height: 1;
  white-space: nowrap;
  opacity: 0;
  animation: autocomplete-in 0.15s ease-out forwards;
}

@keyframes autocomplete-in {
  to {
    opacity: 1;
  }
}

.autocomplete-suggestion {
  color: var(--fg-muted, #666);
  opacity: 0.5;
}

.autocomplete-hint {
  margin-left: 8px;
  padding: 1px 5px;
  font-size: 11px;
  font-family: inherit;
  color: var(--fg-muted, #666);
  background: var(--bg-surface, rgba(255, 255, 255, 0.08));
  border: 1px solid var(--border, rgba(255, 255, 255, 0.12));
  border-radius: 3px;
  opacity: 0.7;
}
</style>
