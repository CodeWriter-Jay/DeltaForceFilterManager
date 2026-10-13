# DeltaForce Filter Manager

面向《三角洲行动》的 Windows 显示器色彩调试工具。**HTML / CSS / TypeScript 界面 + Rust / Tauri 2 后台**，用本机参考图片对照暗部、中间调、高光与 RGB，支持 ICC、NVIDIA 数字振动、手动开启、游戏进程联动和托盘。

**[下载最新版客户端](https://github.com/CodeWriter-Jay/DeltaForceFilterManager/releases/latest)**

- **安装版**：下载 `win-x64-setup.exe`，安装器在缺少 WebView2 时安装共享运行时。
- **便携版**：下载 `win-x64-portable.zip`，解压到固定目录，运行 `DeltaForceFilterManager.exe`。
- **单文件客户端**：下载 `win-x64.exe` 直接运行；需要本机已有 WebView2。建议固定文件名/目录后再设置自启。

支持 Windows 10/11 x64、SDR。便携版和单文件 EXE 使用系统 **Microsoft Edge WebView2**，不捆绑 Chromium、完整 WebView2 或 .NET。客户端下载在 Release 的 **Assets** 中；Actions 中的 artifact 是构建验证包。

## 程序截图

调色工作台：参考图片分屏对照、RGB 曲线和参数调节。

![调色工作台](docs/screenshots/workspace.png)

应用设置：登录启动、托盘、退出恢复和游戏进程联动。

![应用设置](docs/screenshots/settings.png)

紧凑布局：适合较小窗口的工作台。

![紧凑布局](docs/screenshots/compact.png)

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

默认游戏进程名为 `DeltaForceClient-Win64-Shipping.exe`、`DeltaForceClient.exe`，可通过 **选择运行中的进程** 调整；填写多个名称时，任一匹配即可触发。自动模式中手动恢复会暂停本次游戏会话的自动应用，直到游戏退出；自动启用后手动更新仍保留游戏退出时恢复的行为。关闭跟随模式会恢复由自动模式启用的滤镜。

## 运行说明

ICC 的色彩描述与校准曲线不同：有 `vcgt` 时与调整 LUT 叠加，没有时仍关联系统 ICC，但游戏 LUT 使用中性基准；非色彩管理游戏通常不使用 ICC 的矩阵/色域转换。调色基准始终来自启用前备份，方案切换不会累积曲线。高光压缩不能恢复游戏渲染时已裁剪的细节。

亮度、对比度、Gamma 和 RGB 调节使用 Windows Gamma Ramp。NVIDIA 数字振动需要显示器由 NVIDIA 显卡直接输出且驱动提供 DVC 扩展接口；不可用时其余调色功能仍可使用。应用会在启用前备份原始色彩，写入后回读并复查，失败时恢复。

Windows 显示切换、独占全屏、休眠、插拔、夜间模式或其他校色软件可能重置色彩曲线，遇到此情况可重新应用方案。开启 HDR/高级色彩时停止 SDR 应用或恢复。克隆显示器共享 GDI source，无法独立调节。

滤镜作用于整个目标显示器，包括本应用；图片分屏用于相对对照，不能精确模拟面板、ICC 色域转换或 NVIDIA 数字振动。导入 ICC 后首次系统安装可能受权限限制；遇到错误可先在 Windows 色彩管理中安装，应用不会静默提权。恢复原始默认配置不会卸载已安装的文件或删除导入关联。

关闭到托盘时销毁 WebView 窗口，释放界面和图片缓存；再打开时重新创建窗口，保留当前草稿和本次选中的参考图片路径。图片内容不写入设置。

应用不注入游戏或读取游戏内存。

## 设置与恢复

设置和恢复备份保存在 `%LOCALAPPDATA%\DeltaForceFilterManager\`。保持 v0.1.0 的 JSON 格式（PascalCase、SchemaVersion 1）。未完成的旧备份在启动时优先恢复。

损坏文件不会自动重置或覆盖。恢复失败会保留备份，可在重新连接原显示器或关闭 HDR 后重试。关闭“退出时恢复”后仍保留备份，下次启动会恢复。紧急恢复在程序未运行时执行：

```powershell
.\DeltaForceFilterManager.exe --restore
```

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

MIT License。
