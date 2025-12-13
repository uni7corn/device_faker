/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const component: DefineComponent<Record<string, never>, Record<string, never>, any>
  export default component
}

interface ImportMetaEnv {
  readonly DEV: boolean
  readonly PROD: boolean
  readonly MODE: string
  readonly VITE_DEBUG?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

// 扩展 Window 接口
interface Window {
  $packageManager?: {
    getApplicationIcon: (packageName: string, flags: number, userId: number) => unknown
    getApplicationInfo: (
      packageName: string,
      flags: number,
      userId: number
    ) => {
      getLabel: () => string
    }
  }
  wrapInputStream?: (stream: unknown) => Promise<{
    arrayBuffer: () => Promise<ArrayBuffer>
  }>
  ksu?: unknown
}
