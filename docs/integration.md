# 在其他文件管理器中使用 Omni Look

Omni Look 在资源管理器和桌面上按 Ctrl+空格（可改为空格）即可预览。其他文件管理器通过命令行调用同一个预览窗口：

```
omnilook preview <文件> [--title <显示的名称>]
```

- 命令立刻返回，预览窗口出现在屏幕中央，不抢键盘焦点（Windows）。
- **同一个文件再调用一次，预览关闭**。所以把这条命令绑到一个快捷键上，就能像 Ctrl+空格一样"按一下打开、再按一下关闭"。
- 预览打开时调用另一个文件，预览窗口直接换成新文件。
- Windows 上需要的后台进程（代理）不在时会自动启动；Linux 没有后台进程，每次打开一个独立的预览窗口。

下文的 `omnilook.exe` 指安装位置，默认是：

| 系统 | 位置 |
|---|---|
| Windows | `%LOCALAPPDATA%\Omni Look\omnilook.exe`（安装程序同时登记了 `App Paths`，"运行"对话框里直接输入 `omnilook` 也可以） |
| Linux（.deb / .rpm） | `/usr/bin/omnilook` |
| Linux（AppImage） | 你放 AppImage 的位置，例如 `~/Applications/Omni-Look.AppImage` |

## Windows

### Directory Opus

1. 设置 → 自定义工具栏（或"键盘快捷键"），新建一个按钮或快捷键。
2. 函数类型选"标准函数（Opus 或外部）"，内容：

   ```
   "/localappdata\Omni Look\omnilook.exe" preview {filepath}
   ```

3. 给它分配一个快捷键，例如 Ctrl+空格。Opus 自带的查看器占用空格时，可以改用其他键。

### Total Commander

1. 打开 `usercmd.ini`（配置 → 选项 → 杂项 → 重新定义热键 → 编辑 em_ 命令；或直接编辑 Total Commander 配置目录下的文件），加入：

   ```ini
   [em_omnilook]
   cmd="%LOCALAPPDATA%\Omni Look\omnilook.exe"
   param=preview "%P%N"
   menu=用 Omni Look 预览
   ```

2. 配置 → 选项 → 杂项 → 重新定义热键：选一个键（例如 Ctrl+Shift+Q；Ctrl+Q 是 Total Commander 自己的快速查看），命令选 `em_omnilook`。

### XYplorer

1. 用户 → 管理命令 → "运行程序"，新建：

   ```
   "%LOCALAPPDATA%\Omni Look\omnilook.exe" preview "<curitem>"
   ```

2. 在同一个对话框里给它分配快捷键。

### FreeCommander XE

工具 → 收藏工具 → 新建：程序填 `%LOCALAPPDATA%\Omni Look\omnilook.exe`，参数填 `preview "%ActivSel%"`，再在"快捷键"里为它分配按键。

### 其他程序

凡是能"用当前选中的文件运行外部程序"的文件管理器（OneCommander、Files、Q-Dir 等）都可以：程序填 `omnilook.exe`，参数填 `preview "<选中文件的完整路径>"`，路径变量的写法见各程序的说明。

## Linux

Linux 上没有全局快捷键（Wayland 不允许全局键盘钩子），通过文件管理器的右键菜单或自定义动作调用。`omnilook <文件>` 打开一个预览窗口，左右键切换同一文件夹里的文件，Esc 关闭。GNOME 用户也可以继续使用系统自带的 Sushi。

播放音视频需要 GStreamer 插件（.deb 安装包已列为推荐依赖）：`gstreamer1.0-plugins-good`、`gstreamer1.0-plugins-bad`、`gstreamer1.0-libav`。

### Dolphin（KDE）

创建 `~/.local/share/kio/servicemenus/omnilook.desktop`：

```ini
[Desktop Entry]
Type=Service
MimeType=all/allfiles;
Actions=omnilook
X-KDE-ServiceTypes=KonqPopupMenu/Plugin

[Desktop Action omnilook]
Name=用 Omni Look 预览
Icon=omnilook
Exec=omnilook %f
```

Plasma 6 要求这个文件可执行：`chmod +x ~/.local/share/kio/servicemenus/omnilook.desktop`。之后右键 → 操作 → 用 Omni Look 预览。

### Nautilus（GNOME 文件）

创建脚本 `~/.local/share/nautilus/scripts/用 Omni Look 预览`：

```sh
#!/bin/sh
# Nautilus 把选中的文件逐行放在这个变量里，取第一个。
file=$(printf '%s\n' "$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS" | head -n 1)
exec omnilook "$file"
```

`chmod +x` 后右键 → 脚本 → 用 Omni Look 预览。要用快捷键，在 `~/.config/nautilus/scripts-accels` 中加一行（例如 F3），再重启 Nautilus：

```
F3 用 Omni Look 预览
```

### Nemo（Cinnamon）

创建 `~/.local/share/nemo/actions/omnilook.nemo_action`：

```ini
[Nemo Action]
Name=用 Omni Look 预览
Exec=omnilook %F
Icon-Name=omnilook
Selection=s
Extensions=any;
```

### Thunar（Xfce）

编辑 → 配置自定义动作 → 添加：名称"用 Omni Look 预览"，命令 `omnilook %f`，在"出现条件"里勾选所有文件类型。

## 给开发者：接入协议

- **发现（Windows）**：`HKCU\Software\OmniLook` 下的 `Exe`（REG_SZ）是可执行文件位置，`ProtocolVersion`（REG_DWORD）是调用协议版本。版本 ≥ 1 支持上面的 `preview` 命令。
- **标题**：从压缩包、网盘缓存等位置交出临时文件时，用 `--title` 传入用户认得的名称（例如"压缩包名 › 包内路径"），预览窗口显示它而不是临时路径。
- **左右切换（版本 ≥ 2）**：额外传入 `--nav \\.\pipe\<你的管道名>`。用户在预览窗口里按左右键时，Omni Look 连接这个命名管道，发送一行 JSON：`{"op":"next","path":"<当前文件>"}`（`op` 也可能是 `prev`），你回复一行 `{"path":"<下一个文件>","title":"<名称>"}`，没有了就回复 `{"end":true}`。管道应只允许当前用户连接。完整说明见[开发方案](development-plan.md) §6.3；OmniZip 的实现见其仓库的 `apps/desktop/src-tauri/src/look_nav.rs`。
