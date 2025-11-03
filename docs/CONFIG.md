# 配置说明

## 配置文件路径
- `/data/adb/device_faker/config/config.toml`

配置文件使用 TOML 格式

## 完整配置示例

```toml
# 全局默认模式（推荐使用 lite）
default_mode = "lite"

# 调试模式（可选，默认关闭）
# debug = true

# 应用配置
[[apps]]
package = "com.omarea.vtools"
manufacturer = "Xiaomi"
brand = "Xiaomi"
marketname = "Xiaomi 17 Pro Max"
model = "2509FPN0BC"
name = "popsicle"
# mode = "full"  # 可选：覆盖全局默认模式

[[apps]]
package = "com.coolapk.market"
manufacturer = "Nothing"
brand = "Nothing"
marketname = "Nothing Phone (3)"
model = "A024"
```

## 全局设置

### default_mode（全局默认模式）

```toml
default_mode = "lite"  # 推荐：轻量模式（像 COPG 一样）
# default_mode = "full"  # 完整模式（可能被检测）
```

**可选值**：
- `"lite"` - 轻量模式（推荐）⭐
  - 只修改 Build 类静态字段
  - 完成后卸载模块
  - 不易被检测
  - 适合 90% 的应用

- `"full"` - 完整模式
  - 修改 Build 类 + Hook SystemProperties
  - 模块驻留内存
  - 可能被检测
  - 仅在 lite 不够用时使用

### debug（调试模式）

```toml
debug = true  # 启用详细日志（用于调试）
# debug = false  # 或删除此行，默认关闭（正常使用）
```

**说明**：
- 启用后会输出详细的 Info 级别日志
- 关闭时只输出 Error 级别日志
- 正常使用建议关闭以提高隐蔽性

## 应用配置

### 字段说明

所有字段除了 `package` 外都是**可选的**，只配置需要伪装的字段即可：

- `package` (必需): 应用包名
- `manufacturer` (可选): 制造商，伪装 `Build.MANUFACTURER`
- `brand` (可选): 品牌，伪装 `Build.BRAND`
- `marketname` (可选): 市场名称
- `model` (可选): 型号，伪装 `Build.MODEL`
- `name` (可选): 产品名，伪装 `Build.PRODUCT`、`Build.DEVICE`
- `mode` (可选): 此应用的模式（覆盖全局 default_mode）
- `debug` (可选): 此应用的调试开关（覆盖全局 debug）

### 最小配置示例

只伪装部分字段：

```toml
default_mode = "lite"

[[apps]]
package = "com.example.app"
model = "Pixel 9"
brand = "Google"

[[apps]]
package = "com.another.app"
manufacturer = "Samsung"
```

### 混合模式示例

大多数应用用 lite，个别应用用 full：

```toml
default_mode = "lite"  # 默认轻量模式

# 普通应用使用默认 lite 模式
[[apps]]
package = "com.normal.app"
manufacturer = "Xiaomi"
model = "2409FRN0DC"

# 特定应用需要完整伪装
[[apps]]
package = "com.special.app"
manufacturer = "Nothing"
model = "A065"
mode = "full"  # 此应用使用完整模式
```

## 模式对比

| 特性 | lite 模式 ⭐ | full 模式 |
|------|-------------|-----------|
| Build 类伪装 | ✅ | ✅ |
| SystemProperties Hook | ❌ | ✅ |
| 模块可卸载 | ✅ | ❌ |
| 隐蔽性 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| 被检测风险 | 极低 | 中等 |
| 推荐度 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |

## 如何选择模式？

### 推荐策略

1. **默认使用 lite 模式** - 覆盖 90% 的场景
2. **测试应用** - 看是否能正常使用
3. **如果不行** - 再改用 full 模式

### 判断依据

**使用 lite 模式**：
- ✅ 大多数应用
- ✅ 追求隐蔽性
- ✅ 不想被检测

**使用 full 模式**：
- 应用会读取 SystemProperties
- lite 模式下仍能检测到真实机型
- 不在乎被检测到模块
