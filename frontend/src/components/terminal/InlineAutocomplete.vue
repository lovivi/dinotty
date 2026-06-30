<template>
  <div
    v-if="visible && topSuggestion"
    class="inline-autocomplete"
    :style="{
      left: cursorX + 'px',
      top: cursorY + 'px',
      fontSize: fontSize + 'px',
      fontFamily: fontFamily,
    }"
    @mousedown.prevent
  >
    <span class="autocomplete-suggestion">{{ ghostCompletion }}</span>
    <div
      v-if="dropdownRows.length > 0"
      class="autocomplete-dropdown"
      :style="{
        fontSize: fontSize + 'px',
        fontFamily: fontFamily,
        top: lineHeight + 'px',
      }"
    >
      <button
        v-for="(row, i) in dropdownRows"
        :key="row"
        type="button"
        class="autocomplete-row"
        :class="{ 'autocomplete-row-selected': i + 1 === selectedIdx }"
        @mouseenter="$emit('hover', i + 1)"
        @click="$emit('hover', i + 1); $emit('accept')"
      >
        <span class="autocomplete-row-prefix">{{ typedText }}</span><span class="autocomplete-row-rest">{{ rowCompletion(row) }}</span>
        <span class="autocomplete-row-hint">{{ i + 2 }}</span>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  /** Inline ghost suggestion (= suggestions[0]). */
  suggestion: string
  /** Full ordered list. suggestions[0] is the ghost; the rest are dropdown rows. */
  suggestions: string[]
  /** Which dropdown row is highlighted. 0 = inline ghost, 1+ = dropdown row. */
  selectedIdx: number
  typedText: string
  cursorX: number
  cursorY: number
  visible: boolean
  fontSize?: number
  fontFamily?: string
  /** Cell height in px — used to offset the dropdown below the prompt row. */
  lineHeight?: number
}>()

defineEmits<{
  hover: [idx: number]
  accept: []
}>()

const fontSize = computed(() => props.fontSize ?? 14)
const fontFamily = computed(() => props.fontFamily ?? 'monospace')
const lineHeight = computed(() => props.lineHeight ?? (fontSize.value * 1.2))
const topSuggestion = computed(() => props.suggestion || '')

const ghostCompletion = computed(() => {
  if (!topSuggestion.value || !props.typedText) return ''
  if (topSuggestion.value.startsWith(props.typedText)) {
    return topSuggestion.value.substring(props.typedText.length)
  }
  return topSuggestion.value
})

const dropdownRows = computed(() => props.suggestions.slice(1))

function rowCompletion(row: string): string {
  if (!props.typedText) return row
  if (row.startsWith(props.typedText)) {
    return row.substring(props.typedText.length)
  }
  return row
}
</script>

<style scoped>
.inline-autocomplete {
  position: absolute;
  z-index: 100;
  pointer-events: none;
  display: flex;
  align-items: center;
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

.autocomplete-dropdown {
  position: absolute;
  left: 0;
  min-width: 100%;
  max-width: 480px;
  max-height: 240px;
  overflow-y: auto;
  background: var(--bg-surface, rgba(40, 40, 40, 0.96));
  border: 1px solid var(--border, rgba(255, 255, 255, 0.12));
  border-radius: 4px;
  padding: 2px 0;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.35);
  /* Dropdown rows need clicks and hover, the ghost span above doesn't. */
  pointer-events: auto;
}

.autocomplete-row {
  display: flex;
  align-items: center;
  width: 100%;
  padding: 2px 8px;
  border: none;
  background: transparent;
  color: var(--fg-muted, #aaa);
  font: inherit;
  text-align: left;
  cursor: pointer;
  white-space: nowrap;
}

.autocomplete-row:hover,
.autocomplete-row-selected {
  background: var(--bg-active, rgba(255, 255, 255, 0.10));
  color: var(--fg-bright, #eee);
}

.autocomplete-row-prefix {
  color: var(--fg-muted, #888);
}

.autocomplete-row-rest {
  color: var(--fg, #ccc);
  font-weight: 500;
}

.autocomplete-row-selected .autocomplete-row-rest {
  color: var(--accent, #5b9dd9);
}

.autocomplete-row-hint {
  margin-left: auto;
  padding: 0 6px;
  font-size: 0.85em;
  color: var(--fg-muted, #666);
  background: var(--bg, rgba(0, 0, 0, 0.20));
  border-radius: 3px;
}
</style>