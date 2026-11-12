# Omni Look

Windows 上的快速预览：在资源管理器或桌面选中文件，按 **Ctrl+空格**，立刻看到内容；再按一次或 Esc 关闭，方向键切换相邻文件。像 macOS 的 Quick Look，但更轻：常驻后台只占约 2 MB 内存，预览窗口用时启动、关掉即退出。

Rust + Tauri 2 + React，与 [OmniZip](https://github.com/VoidDel/OmniZip)、[Omni Download](https://github.com/VoidDel/OmniDownloadeManager) 同属 Omni 家族，共用设计系统 [omni-ui](https://github.com/VoidDel/omni-ui) 与主题。

## 下载

在 [Releases](https://github.com/VoidDel/OmniLook/releases/latest) 下载：

| 系统 | 文件 | 说明 |
|---|---|---|
| Windows 10 / 11（x64） | `Omni.Look_<版本>_x64-setup.exe` | 为当前用户安装，不需要管理员权限；装好后即在后台待命，并随 Windows 启动 |
| Linux（x64） | `.deb`、`.rpm`、`.AppImage` | 命令行模式，见下文 |

安装包还没有代码签名，Windows SmartScreen 可能提示"未知发布者"，点"更多信息 → 仍要运行"即可。之后的版本会在应用内提示更新（"设置 › 关于"）。

## 使用

| 操作 | 按键 |
|---|---|
| 预览选中的文件 | Ctrl+空格（设置里可改为单独的空格） |
| 关闭 | 再按一次 Ctrl+空格，或 Esc；在文件夹空白处点一下（不再选中任何文件）；切到其他窗口时也会自动关闭（可关闭此行为） |
| 上一个 / 下一个文件 | 方向键（在资源管理器里移动选中项，预览跟着换） |
| 用默认程序打开 | Ctrl+Enter，或标题栏的按钮 |

再次从开始菜单启动 Omni Look 会打开设置：快捷键、开机启动、托盘图标、语言、深浅色与主题、更新与反馈。

## 能预览什么

| 类型 | 内容 |
|---|---|
| 文本与代码 | 自动识别编码（含 GBK、Shift_JIS），上百种语言的语法高亮，几 MB 的日志也能流畅滚动 |
| 固件与链接脚本 | Intel HEX（.hex）与 Motorola S-record（.s19、.srec、.mot 等）按字段着色，逐行检查长度与校验和并标出错误行，状态栏显示数据大小、地址范围与入口地址；GNU ld（.ld）与 IAR（.icf）链接脚本语法高亮 |
| CAD 图纸 | DWG、DXF：模型空间的图形，含块、标注、填充、文字（包括旧图纸的 GBK 编码），颜色随图层；滚轮缩放、拖动平移、双击回到全图，可切换黑色背景。不需要装 AutoCAD |
| 3D 模型 | STEP（.step、.stp）与 BREP 的零件和装配体，带颜色、零件数与毫米尺寸；STL、OBJ（含材质）、PLY、glTF/GLB、3MF、FBX、DAE、3DS、AMF。拖动旋转、右键平移、滚轮缩放，前/顶/右/轴测视图、棱线与黑色背景开关；左侧零件树可展开装配体、隐藏零件，单击（列表或模型上）选中并高亮，双击放大到该零件。大的 STEP 装配体第一次要读半分钟左右，之后再预览很快 |
| SolidWorks、Inventor | .sldprt、.sldasm、.slddrw，.ipt、.iam、.idw、.ipn：显示文件里保存的预览图（新旧两种 SolidWorks 格式都支持）；装了 SolidWorks、Inventor 或 eDrawings 时改用它们的预览程序 |
| Markdown | 渲染后的排版（表格、任务列表、脚注、代码高亮），可切换到源码；不加载远程资源 |
| 图片 | PNG、JPEG、GIF、WebP、BMP、ICO、SVG、AVIF、TIFF；HEIC 需要 Microsoft Store 的"HEIF 图像扩展" |
| PDF、音频、视频 | 内置播放；音频显示封面与标签；放不了的格式（AVI、WMV 等）显示文件信息 |
| 压缩包 | ZIP、7z、RAR、TAR、ISO 等 OmniZip 能读的全部格式，显示目录树、大小与加密情况 |
| Office 等 | Word、Excel、PowerPoint、Outlook 邮件等经系统的预览程序显示；没装 Office 时显示其中的文字与表格 |
| 字体 | TrueType、OpenType、WOFF：名称、字符与多个字号的样句，可输入文字试看 |
| 文件夹与其他 | 文件夹内容与总大小；其余文件显示十六进制 |

## 与 OmniZip 和其他文件管理器

- **OmniZip**：装了 Omni Look 后，OmniZip 里按空格就用 Omni Look 预览压缩包内的文件，左右键切换包内相邻文件（OmniZip"设置 › 常规 › 预览方式"可选）。
- **其他文件管理器**：Directory Opus、Total Commander、XYplorer 等通过 `omnilook preview <文件>` 接入；Linux 的 Dolphin、Nautilus、Nemo、Thunar 用自定义动作调用。见[集成说明](docs/integration.md)。

## Linux

Wayland 不允许全局快捷键，Linux 版是命令行模式：`omnilook <文件>` 打开预览窗口，左右键切换同一文件夹里的文件，Esc 关闭；`omnilook` 打开设置。播放音视频需要 GStreamer 插件（`.deb` 已列为推荐依赖）。

## 隐私与安全

预览窗口不联网，文件不会离开你的电脑；唯一的网络访问是检查更新（可在设置中关闭）。不可信的文件内容在 Rust 中解析和净化，Office 等系统预览程序运行在系统的独立进程里。详见 [SECURITY.md](SECURITY.md)。

## 反馈

问题与建议请提交到 [Issues](https://github.com/VoidDel/OmniLook/issues/new/choose)（"设置 › 关于 › 反馈问题"也能直接打开）。安全问题请用[私密漏洞报告](https://github.com/VoidDel/OmniLook/security/advisories/new)。

## 开发

需要 Rust stable、Node 22+、pnpm 10。

```bash
pnpm install
pnpm dev              # 开发窗口（Vite 端口 1440）
pnpm build            # 安装包，输出在 target/release/bundle/nsis
```

测试与贡献方式见 [CONTRIBUTING.md](CONTRIBUTING.md)；设计见[开发方案](docs/development-plan.md)，原型结果见 [M0 验证结果](docs/m0-results.md)，版本变化见 [CHANGELOG.md](CHANGELOG.md)。

| 路径 | 内容 |
|---|---|
| `apps/desktop/` | Tauri 应用：`src/` 为 React 前端（预览窗口、设置），`src-tauri/` 为 Rust 外壳 |
| `crates/omnilook-core` | 类型识别与各类预览器（平台无关） |
| `crates/omnilook-win` | 键盘钩子、资源管理器选中项、系统预览程序宿主、WIC 解码、命名管道 |
| `locales/` | 简体中文（源）、繁體中文、English、日本語 |
| `docs/` | 开发方案、集成说明与设计文档 |

## 许可

[MIT](LICENSE)。第三方组件中，读取 DWG 与 DXF 的 [acadrust](https://github.com/hakanaktt/acadrust) 以 MPL-2.0 授权，读取 STEP 与 BREP 的 Open CASCADE（[occt-wasm](https://github.com/andymai/occt-wasm)，WebAssembly 部分）以 LGPL-2.1 授权，均未作修改，详见 [licenses/](licenses/README.md)。
