# 参与贡献

欢迎反馈问题、提建议和提交代码。

## 反馈问题

在 [Issues](https://github.com/VoidDel/OmniLook/issues/new/choose) 里选对应的模板（设置窗口的"关于 › 反馈问题"也会打开这里）。问题反馈请写清楚：

- 版本号（"设置 › 关于"）和操作系统。
- 怎么打开的预览：资源管理器 / 桌面的快捷键、OmniZip、命令行或其他文件管理器。
- 出问题的文件类型；方便的话附上能重现问题的样本文件。
- 做了什么、期望什么、实际发生了什么，最好附截图。
- Windows 的日志 `%LOCALAPPDATA%\OmniLook\agent.log`。日志里有文件路径，介意的话先抹掉。

安全问题请不要公开提交，见 [SECURITY.md](SECURITY.md)。

## 搭建开发环境

需要 Rust stable（见 `rust-toolchain.toml`）、Node 22+、pnpm 10，以及各平台的 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)（Linux 上是 `libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`librsvg2-dev`）。

```bash
pnpm install
pnpm dev          # 开发窗口（Vite 端口 1440）
pnpm build        # 安装包（Windows：target/release/bundle/nsis）
```

测试时设置 `OMNILOOK_HOME=<目录>`：设置、WebView 数据与日志都放在那里，不写注册表，管道名带上目录标记，不会碰到正在运行的 Omni Look；隔离的后台进程默认也不装键盘钩子（需要时设 `OMNILOOK_HOOK=1`）。`OMNILOOK_NO_HANDLERS=1` 可以模拟没装 Office 的电脑。

## 代码在哪里

| 路径 | 内容 |
|---|---|
| `apps/desktop/src/` | 界面（React + TypeScript）：预览窗口 `view/`、设置 `settings/` |
| `apps/desktop/src-tauri/` | Rust 外壳：后台代理 `agent.rs`、命令行 `cli.rs`、窗口 `ui/`（`win.rs` 与 `other.rs` 是平台差异）、更新 `update.rs` |
| `crates/omnilook-core` | 类型识别与各类预览器，平台无关 |
| `crates/omnilook-win` | Windows：键盘钩子、资源管理器选中项、系统预览程序宿主、WIC 解码、命名管道 |
| `locales/` | 界面文字：简体中文（源）、繁體中文、English、日本語 |
| `docs/` | [开发方案](docs/development-plan.md)、[集成说明](docs/integration.md)、[M0 验证结果](docs/m0-results.md) |

较大的改动之前，先读开发方案里相关的章节，并开一个 Issue 说明打算怎么做。

## 提交之前

CI 会跑下面这些，本地先过一遍：

```bash
pnpm check:locales                         # 四种语言的键与占位符一致，代码里用到的键都存在
pnpm lint                                  # TypeScript 类型检查与 ESLint
pnpm test                                  # 前端单元测试
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

提交信息用中文，格式参考已有的提交（`feat(m3): …`、`fix: …`、`docs: …`）。

## 发布

推送标签 `v<版本号>`（与 `apps/desktop/src-tauri/tauri.conf.json` 的 `version` 一致）后，GitHub Actions 构建 Windows 与 Linux 安装包和更新清单，放进一个草稿发布；检查后发布草稿，应用内的更新随之可用。
