import { nextTick, ref } from 'vue'
import { wrapInputStream } from 'webuix'
import { normalizePackageName } from '../utils/package'

const ICON_CONTAINER_SELECTOR = '.app-icon-container'

type IconMap = Record<string, string>
type IconLoadedMap = Record<string, boolean>

async function loadWrapInputStream() {
  // 优先使用 webuix 库的 wrapInputStream
  if (typeof window.wrapInputStream === 'undefined') {
    window.wrapInputStream = wrapInputStream
  }
}

function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const uint8Array = new Uint8Array(buffer)
  let binary = ''
  uint8Array.forEach((byte) => (binary += String.fromCharCode(byte)))
  return btoa(binary)
}

export function useAppIcons() {
  const appIcons = ref<IconMap>({})
  const iconLoaded = ref<IconLoadedMap>({})
  const iconObserver = ref<IntersectionObserver | null>(null)

  const onIconLoad = (packageName: string) => {
    iconLoaded.value[packageName] = true
  }

  const onIconError = (packageName: string) => {
    appIcons.value[packageName] = 'fallback'
    iconLoaded.value[packageName] = true
  }

  const loadAppIcon = async (packageName: string) => {
    if (appIcons.value[packageName]) return

    const normalizedPackage = normalizePackageName(packageName)

    try {
      if (typeof window.$packageManager !== 'undefined') {
        const pm = window.$packageManager

        try {
          const stream = pm.getApplicationIcon(normalizedPackage, 0, 0)
          if (!stream) {
            appIcons.value[packageName] = 'fallback'
            return
          }

          await loadWrapInputStream()
          const wrapInputStream = window.wrapInputStream

          if (wrapInputStream) {
            const wrapped = await wrapInputStream(stream)
            const buffer = await wrapped.arrayBuffer()
            const base64 = arrayBufferToBase64(buffer)
            appIcons.value[packageName] = `data:image/png;base64,${base64}`
            return
          }
        } catch {
          // ignore and fallback
        }
      }

      // Check for KSU API
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      if (typeof (globalThis as any).ksu?.getPackagesInfo !== 'undefined') {
        appIcons.value[packageName] = `ksu://icon/${normalizedPackage}`
        return
      }

      appIcons.value[packageName] = 'fallback'
    } catch {
      appIcons.value[packageName] = 'fallback'
    }
  }

  const observeContainers = () => {
    nextTick(() => {
      const containers = document.querySelectorAll(ICON_CONTAINER_SELECTOR)
      containers.forEach((container) => {
        iconObserver.value?.observe(container)
      })
    })
  }

  const setupIconObserver = () => {
    if (iconObserver.value) {
      iconObserver.value.disconnect()
    }

    iconObserver.value = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            const container = entry.target as HTMLElement
            const packageName = container.dataset.package
            if (packageName) {
              loadAppIcon(packageName)
              iconObserver.value?.unobserve(container)
            }
          }
        })
      },
      {
        rootMargin: '100px',
        threshold: 0.1,
      }
    )

    observeContainers()
  }

  const teardownIconObserver = () => {
    if (iconObserver.value) {
      iconObserver.value.disconnect()
      iconObserver.value = null
    }
  }

  return {
    appIcons,
    iconLoaded,
    loadAppIcon,
    onIconLoad,
    onIconError,
    setupIconObserver,
    teardownIconObserver,
  }
}
