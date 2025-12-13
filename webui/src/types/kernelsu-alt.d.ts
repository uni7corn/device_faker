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
