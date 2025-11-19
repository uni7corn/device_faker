// kernelsu-alt 模块的扩展类型声明（如果需要额外类型定义可以放在这里）

// 扩展 globalThis 类型以支持 KernelSU 新的 WebUI 包管理器 API
declare global {
  interface KsuApi {
    getPackagesInfo: (packageName: string) => Promise<{
      appLabel: string
      versionName: string
      versionCode: number
    }>
  }

  var ksu: KsuApi | undefined
}

export {}
