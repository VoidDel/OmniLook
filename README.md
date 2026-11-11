# Omni Look

Windows 上的快速预览：在资源管理器或桌面选中文件按空格，立即预览；再按空格或 Esc 关闭，方向键切换相邻文件。支持文本与代码高亮、图片、压缩包目录树，以及 PDF、音视频和 Office（经系统预览程序）。Rust + Tauri 2 + React，与 [OmniZip](https://github.com/VoidDel/OmniZip)、[Omni Download](https://github.com/VoidDel/OmniDownloadeManager) 同属 Omni 家族，共用设计系统 [omni-ui](https://github.com/VoidDel/omni-ui)。

装了 Omni Look 时，OmniZip 中预览压缩包里的文件也交给它显示。

> 当前进度：**规划阶段**，尚无可运行的代码。方案、架构与路线图见 [开发方案](docs/development-plan.md)。

## 规划的目录

| 路径 | 内容 |
|---|---|
| `apps/desktop/` | Tauri 应用：`src/` 为 React 前端（预览窗口、设置），`src-tauri/` 为 Rust 外壳 |
| `crates/omnilook-core` | 类型识别与各类预览器（平台无关） |
| `crates/omnilook-win` | 键盘钩子、资源管理器集成、系统预览程序宿主 |
| `locales/` | 简体中文（源）、繁體中文、English、日本語 |
| `themes/` | omni-ui 内置主题的副本 |
| `docs/` | 开发方案与设计文档 |

## 许可

[MIT](LICENSE)
