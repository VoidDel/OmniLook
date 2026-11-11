# Omni Look

Windows 上的快速预览：在资源管理器或桌面选中文件按 Ctrl+空格（可改为单独的空格），立即预览；再按一次或 Esc 关闭，方向键切换相邻文件。支持文本与代码高亮、图片、压缩包目录树，以及 PDF、音视频和 Office（经系统预览程序）。Rust + Tauri 2 + React，与 [OmniZip](https://github.com/VoidDel/OmniZip)、[Omni Download](https://github.com/VoidDel/OmniDownloadeManager) 同属 Omni 家族，共用设计系统 [omni-ui](https://github.com/VoidDel/omni-ui)。

装了 Omni Look 时，OmniZip 中预览压缩包里的文件也交给它显示。

> 当前进度：**M1、M2 已完成（2026-11-11）**。M1：常驻代理与快捷键（失去焦点自动关闭）、按需启动的预览窗口、文本与代码高亮、图片、PDF、视频、压缩包目录树、文件夹、十六进制、设置与托盘、NSIS 安装包、OmniZip 接入。M2：Markdown 渲染（可切换源码）、音频封面与标签、系统预览程序（Word、Excel、PowerPoint、字体等）、OmniZip 压缩包内左右切换（协议 v2）。方案见 [开发方案](docs/development-plan.md)，原型结果见 [M0 验证结果](docs/m0-results.md)。

## 开发

需要 Rust stable、Node 22+、pnpm 10。

```bash
pnpm install
pnpm dev              # 开发窗口（Vite 端口 1440）
pnpm build            # 安装包，输出在 target/release/bundle/nsis
```

测试时设置 `OMNILOOK_HOME=<目录>`：设置、WebView2 配置与日志都放在那里，也不写注册表。

## 目录

| 路径 | 内容 |
|---|---|
| `apps/desktop/` | Tauri 应用：`src/` 为 React 前端（预览窗口、设置），`src-tauri/` 为 Rust 外壳 |
| `crates/omnilook-core` | 类型识别与各类预览器（平台无关） |
| `crates/omnilook-win` | 键盘钩子、资源管理器选中项与选中变化、命名管道 |
| `locales/` | 简体中文（源）、繁體中文、English、日本語 |
| `docs/` | 开发方案与设计文档 |

## 许可

[MIT](LICENSE)
