# 安全

Omni Look 会打开任何被选中的文件，所以把解析不可信内容当作头等问题（[开发方案](docs/development-plan.md) §8）：

- 文本、Markdown、压缩包、Office 文件、字体与 TIFF 在 Rust 中解析；页面只拿到净化后的结果（文本、语法区间、数据、PNG），Markdown 经 ammonia 去掉脚本、样式和事件。
- 预览窗口不联网：CSP 禁止外部连接，Markdown 与 SVG 里的远程资源不加载。
- 系统预览程序（Office 等）在系统的代理进程 `prevhost.exe` 中运行，不加载进 Omni Look。
- 进程之间的命名管道只允许当前用户连接，客户端会核对服务端进程属于同一用户。
- 应用内更新只安装用 Omni Look 签名密钥签过的包。

## 报告漏洞

请不要在公开的 Issue 里报告安全问题。使用 GitHub 的 [私密漏洞报告](https://github.com/VoidDel/OmniLook/security/advisories/new)，写明受影响的版本、重现步骤和可能的影响。我们会尽快回复，修复发布前不会公开细节。
