# DeltaForce Filter Manager

面向《三角洲行动》的 Windows 显示器色彩调试工具。原生暗色 GUI，可用游戏截图对照调节暗部、中间调、高光和色彩，支持手动开启、游戏进程联动和托盘运行。

使用 **C# / .NET 10 + WinForms + Win32/NVAPI**，无 WebView、Electron、后台网络请求或第三方运行时包依赖。手动模式不检测进程；跟随模式默认每 3 秒只查询指定进程名；最小化到托盘时释放参考图片缓存。

## 功能

| 功能 | 实现 |
| --- | --- |
| ICC 管理 | 扫描系统与本机导入目录，导入/导出 `.icc` / `.icm`，WCS 当前用户默认配置切换 |
| 显示器调色 | 亮度 −125…125、对比度 −82…82、Gamma 0.4…2.8、RGB 增益 −100…100 |
| NVIDIA 数字振动 | 0…100%，精确匹配显示器，使用驱动实际范围并回读校验；可选，不支持时返回错误 |
| 明暗优化 | 暗部提升、高光压缩；内置均衡、室内、室外、中性起始方案 |
| 参考图片 | 导入或拖入 PNG/JPEG/BMP；仅本机读取；分屏原图/调整后对照，鼠标或方向键移动分隔线 |
| 曲线预览 | 图片使用与显示器相同的 RGB LUT；同时显示 RGB 曲线；数字振动不做图像模拟 |
| 手动与自动模式 | 一键启用；可选跟随指定 `.exe`，最后一个匹配进程退出后恢复原始状态 |
| 自启与托盘 | 可选随 Windows 登录进入托盘；关闭窗口进入托盘；可选退出时恢复 |
| 方案管理 | 保存、另存、删除、JSON 导入/导出；跨电脑导入后重新选择本机 ICC |
| 原始状态恢复 | 启用前持久化备份 ICC 状态、Gamma Ramp、可用的 NVIDIA 数字振动；应用失败回滚；异常退出后下次启动尝试恢复 |
| 多显示器 | 选择具体显示器；备份以显示器身份匹配；恢复前不允许切换调节目标 |
| 快捷键 | `Ctrl+Shift+F10` 手动开关；`Ctrl+Shift+F11` 恢复；占用冲突会提示 |

## 运行

支持 Windows 10/11 **x64、SDR**。NVIDIA 数字振动需要显示器由 NVIDIA 显卡直接输出，并且驱动提供相应 NVAPI 接口；其他显卡可使用 ICC 和 Gamma 调色。

1. 在 [Releases](https://github.com/CodeWriter-Jay/DeltaForceFilterManager/releases) 下载 Windows x64 压缩包。解压到固定目录，运行 `DeltaForceFilterManager.exe`。自包含版本不需另装 .NET。
2. 选择显示器和起始方案，导入一张包含暗部与天空亮部的游戏截图。
3. 先适度调整 Gamma / 暗部提升，再调整高光压缩。观察黑阶是否可区分、亮部是否过灰，最后调 RGB 和可选数字振动。
4. 点击 **开启 / 应用滤镜**，用 **A/B 原始对照**比较整个显示器效果；点击 **保存当前方案**保留参数。
5. 如需自动运行，在应用设置选择真实游戏进程和自动应用方案，保存后打开左侧的 **跟随游戏进程自动开启**。
6. 开机启动在设置中单独启用。更换程序目录后，需要重新保存自启设置。

内置进程名 `DeltaForceClient-Win64-Shipping.exe` 和 `DeltaForceClient.exe` 只是候选，不同发行平台和版本可能不同。请在游戏运行时通过 **选择运行中的进程**确认真实名称。多个名称是“任一匹配”，不是启动器和游戏的先后顺序规则。

自动模式中的手动恢复会保持到当前游戏退出，不会被下一次检测立即覆盖。进程开始时只应用一次，不持续抢占其他校色软件；如果自动应用失败，修复原因后手动应用或重新启动游戏。

## 色彩行为和边界

- **ICC 的色彩描述与校准曲线是两件事。** 有 `vcgt` 的 ICC 会把校准曲线和调节 LUT 叠加；没有 `vcgt` 的 ICC 仍关联到 Windows，但游戏 LUT 使用中性基准。非色彩管理游戏通常不会使用 ICC 内的矩阵/色域转换。
- 亮度、对比度、Gamma 与 RGB 通过 Windows Gamma Ramp 实现，与参考项目当前源码采用同类路径；数字振动通过 NVAPI。参数不是 NVIDIA 控制面板亮度/对比度数值的精确复制，也不调用旧的 `NvCpl.dll` 命令。
- 高光压缩降低已有亮部的输出；无法恢复游戏渲染时已经裁剪掉的细节。预设只作为起点，不能保证适合所有显示器。
- HDR/高级色彩开启时停止应用 SDR 曲线。独占全屏、分辨率变化、休眠、显示器插拔、系统夜间模式、其他校色软件可能重置或覆盖 LUT；必要时重新应用。软件不注入游戏、不读取游戏内存，但未验证特定游戏版本的反作弊兼容性。
- Gamma Ramp 写入返回成功仍可能被驱动静默拒绝，因此会读回验证（允许 8 位 DAC 的量化误差）。曲线过激被拒绝时会回滚并提示。
- 全屏滤镜会作用于整个目标显示器，包括本应用。图片预览用于相对对照，不能代替测色，也不能精确模拟显示器面板、ICC 色域转换或 NVIDIA 数字振动。
- 使用 NVIDIA 数字振动时必须先获取其原始备份；当前会话缺少该备份时需先恢复再启用。显示器由核显输出时不会误调另一台 NVIDIA 输出。
- 克隆显示器共享一个 GDI source，不能单独调节克隆输出。
- 导入 ICC 先保存到当前用户目录。首次应用若 Windows 拒绝系统安装，先通过 Windows 色彩管理安装该文件，再重试。恢复原始默认配置不会卸载已安装的 ICC 文件，也保留导入文件的关联。
- 正常退出可以恢复；强制结束/断电时备份只能在下次启动恢复。若原显示器断开、驱动改变或 HDR 开启，恢复可能暂停；备份保留，重新连接/关闭 HDR 后点击恢复。
- 关闭“退出时恢复”后滤镜可以继续生效，但不会继续监视游戏；下次启动仍尝试恢复备份。参考图片不持久化、不上传。

设置和恢复备份位于 `%LOCALAPPDATA%\DeltaForceFilterManager\`。遇到配置损坏不会静默覆盖。应先保留原文件再处理。紧急恢复可在程序未运行时执行：

```powershell
.\DeltaForceFilterManager.exe --restore
```

## 开发与验证

需要 [.NET 10 SDK](https://dotnet.microsoft.com/download/dotnet/10.0)。不依赖 NuGet 第三方库；Windows Desktop 目标引用在恢复时获取。

```powershell
dotnet run --project tests/DeltaForceFilter.Core.Tests -c Release
dotnet build src/DeltaForceFilter.App -c Release
dotnet run --project src/DeltaForceFilter.App
dotnet publish src/DeltaForceFilter.App -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true -p:DebugType=None -p:DebugSymbols=false -o artifacts/portable
```

macOS/Linux 可运行核心测试、交叉编译 Windows 应用；不能执行 Windows GUI 或硬件接口。自包含包包含 .NET Desktop Runtime，发布包大小与后台工作集是不同指标；尚未在真实游戏电脑测量 CPU、内存或帧率变化，不承诺固定占用。

GitHub Actions 自动运行核心测试、Windows 构建、自包含发布和原生 GUI 冒烟测试，上传 `DeltaForceFilterManager-win-x64` artifact。GUI 测试验证 Win32 结构布局、参考图片导入及 Gamma 预览、源图片无锁定，生成工作台/设置/紧凑窗口截图和手动模式短时空闲资源采样。使用临时配置，不应用滤镜、不写自启注册表。CI 采样不代表游戏电脑性能；硬件色彩效果、ICC 安装权限、真实 NVIDIA 驱动、游戏进程联动和实际占用需在目标 Windows PC 上验证。

测试覆盖曲线端点与单调性、RGB 与校准叠加、ICC 表/公式和恶意边界、参数验证、原始状态事务、部分失败回滚、恢复文件保留、A/B 和进程状态。

```text
src/DeltaForceFilter.Core/   平台无关的参数、曲线、ICC、恢复事务与存储
src/DeltaForceFilter.App/    原生 GUI、图片对照、WCS/GDI/NVAPI、自启与托盘
tests/                     可跨平台运行的核心测试，无额外测试包
.github/workflows/         Windows 构建、GUI 冒烟测试与便携包
```

## 参考依据

功能研究参考 [q974491089/filter-manage](https://github.com/q974491089/filter-manage)，研究版本 `26a563b75c59270837638355b94337e7115fb4b3`（2026-10-12 读取）。该项目除 README 所列 ICC、NVIDIA、保存方案、图片预览外，当前源码也已有 RGB 增益、多显示器、进程规则、自启与快捷键。本项目独立实现并采用原生 WinForms 界面，未复制参考项目的 UI、预设素材或 ICC 文件。

API 依据：[Microsoft WCS](https://learn.microsoft.com/windows/win32/wcs/profile-management-functions)、[Gamma Ramp 限制](https://learn.microsoft.com/windows/win32/api/wingdi/nf-wingdi-setdevicegammaramp)、[NVIDIA NVAPI](https://github.com/NVIDIA/nvapi)。DVC 扩展接口不在公共 NVAPI SDK 的稳定承诺内，签名交叉参考 [NvAPIWrapper](https://github.com/falahati/NvAPIWrapper) 和 [NVIDIA_NvAPI](https://github.com/jNizM/NVIDIA_NvAPI)，不支持时明确报错。

MIT License。
