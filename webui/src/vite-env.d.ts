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

// kernelsu-alt 模块类型声明
declare module 'kernelsu-alt' {
  export interface ExecResult {
    errno: number
    stdout: string
    stderr: string
  }

  export interface PackageInfo {
    appLabel: string
    versionName: string
    versionCode: number
  }

  export function exec(command: string): Promise<ExecResult>

  export function listPackages(type: 'user' | 'system'): Promise<string[]>

  export function getPackagesInfo(packageName: string): Promise<PackageInfo>

  export function toast(message: string): void

  export function spawn(
    command: string,
    args: string[],
    options?: { env?: Record<string, string> }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ): any
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
  ksu?: {
    getPackagesInfo: (packageName: string) => Promise<{
      appLabel: string
      versionName: string
      versionCode: number
    }>
  }
}
