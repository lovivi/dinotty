/**
 * Web Speech API 语音转文字 Composable
 *
 * 封装浏览器原生 SpeechRecognition，提供语音输入能力。
 * 不需要注册/API key，依赖浏览器内置语音引擎。
 *
 * 浏览器支持:
 *   - Chrome 桌面/Android ✅ (完整支持，interimResults)
 *   - Safari iOS/macOS ⚠️ (部分支持，无 interimResults)
 *   - Firefox ❌
 *   - Tauri WebView ⚠️ (需要 HTTPS 且可能被限制)
 *
 * @see https://developer.mozilla.org/en-US/docs/Web/API/SpeechRecognition
 */
import { ref } from 'vue'

export type SpeechRecognitionState =
  | 'unsupported'
  | 'idle'
  | 'recording'
  | 'processing'
  | 'error'

export interface UseSpeechRecognitionReturn {
  readonly state: ReturnType<typeof ref<SpeechRecognitionState>>
  readonly transcript: ReturnType<typeof ref<string>>
  readonly interimTranscript: ReturnType<typeof ref<string>>
  readonly error: ReturnType<typeof ref<string | null>>
  readonly supported: ReturnType<typeof ref<boolean>>

  start: (lang?: string) => void
  stop: () => Promise<string>
  toggle: (lang?: string) => void
  reset: () => void
  destroy: () => void
}

// ── Web Speech API 类型声明 ──
// 标准 TypeScript lib.dom 中包含 SpeechRecognition 类型，
// 但部分构建环境（如 Tauri）可能缺失。此处用 any 规避。
function getSpeechRecognition(): any {
  if (typeof window === 'undefined') return null
  return (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition || null
}

export function useSpeechRecognition(): UseSpeechRecognitionReturn {
  const state = ref<SpeechRecognitionState>('idle')
  const transcript = ref('')
  const interimTranscript = ref('')
  const error = ref<string | null>(null)

  const SRClass = getSpeechRecognition()
  const supported = ref(!!SRClass)

  let recognition: any = null
  let isRecording = false
  let resolveStop: ((text: string) => void) | null = null
  let rejectStop: ((err: Error) => void) | null = null

  function ensureRecognition(): any {
    if (recognition) return recognition
    if (!SRClass) return null

    recognition = new SRClass()
    recognition.continuous = true
    recognition.interimResults = true
    recognition.lang = 'zh-CN'
    recognition.maxAlternatives = 1

    recognition.onresult = (event: any) => {
      let final = ''
      let interim = ''

      for (let i = event.resultIndex; i < event.results.length; i++) {
        const result = event.results[i]
        if (result.isFinal) {
          final += result[0].transcript
        } else {
          interim += result[0].transcript
        }
      }

      if (final) {
        transcript.value += final
        interimTranscript.value = ''
      }
      if (interim) {
        interimTranscript.value = interim
      }
    }

    recognition.onerror = (event: any) => {
      if (event.error === 'aborted') return

      if (event.error === 'no-speech') {
        // 未检测到语音：静默停止，不视为错误
        stopInternal(false)
        return
      }

      error.value = event.error
      state.value = 'error'
      if (rejectStop) {
        rejectStop(new Error(`Speech recognition error: ${event.error}`))
        resolveStop = null
        rejectStop = null
      }
    }

    recognition.onend = () => {
      isRecording = false
      if (state.value === 'recording') {
        // 录音意外结束（Chrome 60s 超时等）
        state.value = 'processing'
        if (transcript.value) {
          if (resolveStop) {
            resolveStop(transcript.value)
            resolveStop = null
            rejectStop = null
          }
        } else if (!error.value) {
          state.value = 'idle'
        }
      } else if (state.value === 'processing') {
        // 正常结束（手动 stop 后）
        state.value = transcript.value ? 'idle' : 'idle'
        if (resolveStop) {
          resolveStop(transcript.value)
          resolveStop = null
          rejectStop = null
        }
      }
    }

    return recognition
  }

  function start(lang?: string) {
    if (isRecording) return
    if (!supported.value) {
      state.value = 'unsupported'
      error.value = '浏览器不支持语音识别，请使用 Chrome'
      return
    }

    const sr = ensureRecognition()
    if (!sr) {
      state.value = 'unsupported'
      error.value = '无法创建 SpeechRecognition 实例'
      return
    }

    transcript.value = ''
    interimTranscript.value = ''
    error.value = null
    state.value = 'recording'
    isRecording = true

    if (lang) sr.lang = lang

    try {
      sr.start()
    } catch {
      state.value = 'error'
      error.value = '录音启动失败'
      isRecording = false
    }
  }

  function stopInternal(manual: boolean) {
    if (!recognition || !isRecording) return
    try {
      recognition.stop()
    } catch {
      // 忽略 stop 错误
    }
    isRecording = false
    if (manual) {
      state.value = 'processing'
    }
  }

  function stop(): Promise<string> {
    return new Promise((resolve, reject) => {
      if (!recognition || !isRecording) {
        resolve(transcript.value || '')
        return
      }

      resolveStop = resolve
      rejectStop = reject

      state.value = 'processing'
      try {
        recognition.stop()
      } catch {
        isRecording = false
        state.value = transcript.value ? 'idle' : 'error'
        resolve(transcript.value || '')
        resolveStop = null
        rejectStop = null
      }
    })
  }

  function toggle(lang?: string) {
    if (isRecording) {
      stopInternal(true)
    } else {
      start(lang)
    }
  }

  function reset() {
    transcript.value = ''
    interimTranscript.value = ''
    error.value = null
    if (state.value === 'error' || state.value === 'idle' || state.value === 'unsupported') {
      state.value = 'idle'
    }
  }

  function destroy() {
    if (recognition && isRecording) {
      try {
        recognition.abort()
      } catch {
        // ignore
      }
    }
    isRecording = false
    recognition = null
    if (resolveStop) {
      resolveStop(transcript.value || '')
      resolveStop = null
      rejectStop = null
    }
  }

  return {
    state,
    transcript,
    interimTranscript,
    error,
    supported,
    start,
    stop,
    toggle,
    reset,
    destroy,
  }
}
