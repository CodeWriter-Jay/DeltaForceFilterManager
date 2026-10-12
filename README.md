# DeltaForce Filter Manager

面向《三角洲行动》的 Windows 显示器色彩调试工具。**HTML / CSS / TypeScript 界面 + Rust / Tauri 2 后台**，用本机参考图片对照暗部、中间调、高光与 RGB，支持 ICC、NVIDIA 数字振动、手动开启、游戏进程联动和托盘。

**[下载最新版客户端](https://github.com/CodeWriter-Jay/DeltaForceFilterManager/releases/latest)**

- **安装版**：下载 `win-x64-setup.exe`，安装器在缺少 WebView2 时安装共享运行时。
- **便携版**：下载 `win-x64-portable.zip`，解压到固定目录，运行 `DeltaForceFilterManager.exe`。
- **单文件客户端**：下载 `win-x64.exe` 直接运行；需要本机已有 WebView2。建议固定文件名/目录后再设置自启。

支持 Windows 10/11 x64、SDR。便携版和单文件 EXE 使用系统 **Microsoft Edge WebView2**，不捆绑 Chromium、完整 WebView2 或 .NET。客户端下载在 Release 的 **Assets** 中；Actions 中的 artifact 是构建验证包。

## v0.2.0 的变化

- 替换 WinForms，采用 HTML 工作台：参考图片左右分屏、RGB 曲线、参数面板、方案与设置页；无远程字体或前端资源。
- 替换 .NET 后台，使用 Rust + Win32/WCS/NVAPI。**v0.1.0 的 47,410,780 字节 ZIP 捆绑了完整 .NET Desktop Runtime**，其中单文件 EXE 约 111 MiB；这不是色彩算法本身的大小。新版共享系统 WebView2，实际客户端大小见 Release。
- 修复本版与参考项目 [issue #8](https://github.com/q974491089/filter-manage/issues/8) 症状相关的缺陷：启用时切换方案会真正应用完整参数；ICC/NVIDIA 操作先完成，最后一次写入整套 RGB LUT；即时回读后，在 200、400、800 ms 处复查并有限重试，再等 400 ms 做最终确认。失败时恢复原始状态，恢复失败保留备份并阻止继续应用。
- 保持 v0.1.0 的设置与恢复备份 JSON 格式（PascalCase、SchemaVersion 1）。未完成的旧备份在启动时优先恢复。
- 关闭到托盘时**销毁 WebView 窗口**，释放界面和图片缓存；再打开重新创建窗口，保留当前草稿和本次选中的参考图片路径。图片内容不写入设置。

issue #8 没有经过确认的根因。本项目回归测试模拟了“写入成功后曲线延迟被重置、数字振动仍保留”的情况，修复的是本项目实际存在的应用顺序、切换和校验缺陷；尚未在问题报告者的显示器/驱动上验证。

## 功能与使用

| 功能 | 行为 |
| --- | --- |
| ICC 管理 | 扫描系统与本机导入目录，导入/导出 `.icc` / `.icm`，切换 WCS 当前用户默认配置 |
| 显示器调色 | 亮度 −125…125、对比度 −82…82、Gamma 0.4…2.8、RGB 增益 −100…100 |
| 明暗调节 | 暗部提升、高光压缩；内置均衡、室内、室外、中性起始方案 |
| NVIDIA 数字振动 | 可选 0…100%，匹配具体显示器，使用驱动真实范围，写后回读 |
| 参考图片 | 选择/拖入 PNG、JPEG、BMP，最大 25 MB / 3200 万像素；本机处理，不上传；箭头键/鼠标移动分隔线 |
| 方案管理 | 保存、另存、删除、JSON 导入/导出；跨电脑导入后重新选择本机 ICC；切换前处理未保存草稿 |
| 手动 / 游戏联动 | 手动模式不查询进程；联动模式默认每 3 秒查询一次进程列表，最后一个匹配进程退出后恢复 |
| 自启 / 托盘 | 设置中独立开关随登录进入托盘、关闭到托盘、退出时恢复；托盘打开/开关/恢复/退出 |
| 事务与恢复 | 首次修改前持久化 ICC、Gamma 和可用的 NVIDIA 原始状态；失败回滚；异常退出下次启动恢复 |
| 多显示器 | 按显示器身份定位；恢复前不允许更换调节目标；断开时保留备份 |
| 快捷键 | `Ctrl+Shift+F10` 开关、`Ctrl+Shift+F11` 恢复；冲突会显示错误 |

1. 选择显示器与起始方案，导入一张包含室内暗部与室外亮部的游戏截图。
2. 适度调整 Gamma 和暗部，再看高光，最后调 RGB 和可选数字振动。点击 **开启滤镜 / 应用当前调整** 写入显示器。
3. 用 **A/B 原始对照** 比较实际显示器效果；点击 **保存方案** 保留参数。启用状态下切换已保存方案会立即应用。
4. 如需自动启用，在设置中通过 **选择运行中的进程** 确认真实游戏进程，指定自动方案，再切换 **跟随游戏**。
5. 需要开机启动时，在设置中开启 **随 Windows 登录启动** 并保存。更换 EXE 名称/目录后重新保存此项。

默认 `DeltaForceClient-Win64-Shipping.exe`、`DeltaForceClient.exe` 仅是候选名，可能随平台/版本变化；多个名称是“任一匹配”。自动模式中手动恢复会暂停本次游戏会话的自动应用，直到游戏退出；自动启用后手动更新仍保留游戏退出时恢复的行为。关闭跟随模式会恢复由自动模式拥有的滤镜。没有永久高频校色线程。

## 色彩行为与验证范围

ICC 的色彩描述与校准曲线不同：有 `vcgt` 时与调整 LUT 叠加，没有时仍关联系统 ICC，但游戏 LUT 使用中性基准；非色彩管理游戏通常不使用 ICC 的矩阵/色域转换。调色基准始终来自启用前备份，方案切换不会累积曲线。高光压缩不能恢复游戏渲染时已裁剪的细节。

亮度/对比度/Gamma/RGB 使用 Windows Gamma Ramp，与参考项目同类路径；不是 NVIDIA 控制面板数值的精确复制。NVIDIA 数字振动需要显示器由 NVIDIA 显卡直接输出且驱动提供 DVC 扩展接口；不可用时其余调色功能仍可使用。启用数字振动前必须有原始备份。

**延迟复查只覆盖有限的应用稳定窗口。** Windows 显示切换、独占全屏、休眠、插拔、夜间模式或其他校色软件仍可能在更晚时间覆盖 LUT，需要重新应用。驱动返回成功也可能静默拒绝，因此读回检查允许 8 位 DAC 的量化误差。开启 HDR/高级色彩时停止 SDR 应用或恢复。克隆显示器共享 GDI source，无法独立调节。

滤镜作用于整个目标显示器，包括本应用；图片分屏用于相对对照，不能精确模拟面板、ICC 色域转换或 NVIDIA 数字振动。导入 ICC 后首次系统安装可能受权限限制；遇到错误可先在 Windows 色彩管理中安装，应用不会静默提权。恢复原始默认配置不会卸载已安装的文件或删除导入关联。

设置和恢复备份：`%LOCALAPPDATA%\DeltaForceFilterManager\`。损坏文件不会静默重置或覆盖。恢复失败会保留备份；重新连接原显示器/关闭 HDR 后重试。关闭“退出时恢复”后仍保留备份，下次启动会恢复。紧急恢复在程序未运行时执行：

```powershell
.\DeltaForceFilterManager.exe --restore
```

不注入游戏或读取游戏内存。**CI 使用模拟显示器验证事务与界面，没有验证真实 NVIDIA 驱动、游戏全屏、反作弊兼容性或游戏电脑帧率。** Release 附空闲采样，包含主进程与 WebView2 子进程；托盘数据是在销毁 WebView 后采样，不能用主进程数字代替 GUI 总内存。

## 开发与验证

需要 Node.js 22.12+、Rust stable；Windows 构建需要 MSVC C++ 工具和 WebView2。使用 lockfile 固定依赖。

```powershell
npm ci
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --locked -- -D warnings
npm run build
npx playwright install chromium
npm run test:web
npm run tauri -- dev
npm run tauri -- build --target x86_64-pc-windows-msvc
```

便携 EXE：`target/x86_64-pc-windows-msvc/release/delta-force-filter-manager.exe`；安装版：相邻 `bundle/nsis/`。macOS 可测试核心和预览 HTML，但硬件调节只在 Windows 实现。浏览器运行 `npm run dev` 会明确标注界面预览，不执行系统调色。

GitHub Actions 验证 Rust 核心/控制器、HTML 交互、方案切换、参考图片与原生 WebView IPC，并生成完整/紧凑/设置截图及进程树空闲报告。仓库维护者手动运行工作流、勾选 `publish_release` 后，验证通过会发布带 EXE、ZIP、安装包、SHA256 校验和的普通 Release，并设为 latest。

```text
crates/filter-core/   参数、曲线、ICC、图片边界、事务、WCS/GDI/NVAPI、自启与进程
src-tauri/           Rust 控制器、HTML IPC、托盘、快捷键与窗口生命周期
web/                 本地 HTML/CSS/TypeScript 工作台
scripts/             图标源、Windows 原生验证与进程树资源采样
tests/web/           HTML 交互与视觉验证
```

## 参考依据

功能研究参考 [q974491089/filter-manage](https://github.com/q974491089/filter-manage)，研究版本 `26a563b75c59270837638355b94337e7115fb4b3`。独立实现，未复制其 UI、图片或 ICC 素材。

API 依据：[Microsoft WCS](https://learn.microsoft.com/windows/win32/wcs/profile-management-functions)、[Gamma Ramp 的平台限制](https://learn.microsoft.com/windows/win32/api/wingdi/nf-wingdi-setdevicegammaramp)、[Tauri Windows 打包与 WebView2](https://v2.tauri.app/distribute/windows-installer/)、[NVIDIA NVAPI](https://github.com/NVIDIA/nvapi)。DVC 扩展接口未获得公共 NVAPI SDK 稳定承诺，签名交叉参考 [NvAPIWrapper](https://github.com/falahati/NvAPIWrapper) 与 [NVIDIA_NvAPI](https://github.com/jNizM/NVIDIA_NvAPI)，不支持时明确报错。

MIT License。
