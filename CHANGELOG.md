# 更新记录

## v0.1.0（2026-11-11）

第一个公开版本。

### Windows

- 在资源管理器或桌面选中文件按 **Ctrl+空格**（可改为空格）预览，再按一次或 Esc 关闭；方向键或鼠标点选其他文件时预览跟着换；切到其他窗口时自动关闭（可在设置中关闭）。
- 常驻后台只占约 2 MB 内存；预览窗口每次单独启动，关闭即退出，只有视频启用 GPU。
- 预览：
  - 文本与代码（自动识别编码，含 GBK、Shift_JIS），语法高亮，大文件流畅滚动；
  - Markdown 渲染（表格、任务列表、脚注、代码高亮），可切换到源码；SVG 也可切换源码；
  - 图片，包括 TIFF、AVIF，以及装有 HEIF 扩展时的 HEIC；
  - PDF、音频（封面与标签）、视频；放不了的格式显示文件信息；
  - 压缩包目录树（OmniZip 能读的全部格式）、文件夹内容、其他文件的十六进制；
  - 字体（TrueType、OpenType、WOFF）：名称、字符与多个字号的样句；
  - Word、Excel、PowerPoint 等经系统预览程序显示；没装 Office 时显示其中的文字与表格。
- 与 OmniZip 集成：OmniZip 里按空格用 Omni Look 预览压缩包内的文件，左右键切换包内相邻文件。
- 命令行 `omnilook preview <文件>`，供 Directory Opus、Total Commander 等文件管理器接入（见[集成说明](docs/integration.md)）。
- 设置：快捷键、开机启动、托盘图标、语言（简体中文、繁體中文、English、日本語）、深浅色与主题；应用内检查与安装更新；反馈问题入口。

### Linux

- 命令行模式：`omnilook <文件>` 打开预览窗口，左右键切换同一文件夹的文件，由 Dolphin、Nautilus、Nemo、Thunar 的自定义动作调用。
- 提供 .deb、.rpm 与 AppImage。
