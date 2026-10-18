# Endfield Launcher / 终末地 Rust 启动器

Windows x64 prerelease. Keep the launcher outside the official installation folders.
Administrator permission is required; double-click the EXE and confirm Windows UAC.

Windows x64 预发布版。请放在官方安装目录之外，双击 EXE 并确认管理员权限提示。

By default, it checks/reuses or downloads official Games.exe, starts the game, then opens the command console.
Official binaries and game files are not included in this archive. The game must already be installed.

默认检查、复用或获取官方 Games.exe，启动游戏后进入命令终端。包内不包含官方程序或游戏文件，需要已有游戏安装。

For a different game location, run from an administrator PowerShell:
游戏路径不同时，在管理员 PowerShell 中执行：

```powershell
.\endfield-direct-launch.exe --game 'E:\Games\Endfield\Endfield.exe' --menu
```

Type `start` to launch, `restart` to restart, `stop` or Ctrl+Z to stop the owned game,
`cn` to replay previous logs in Chinese, and `help` for all commands.
Tab / Shift+Tab cycle completion candidates; Enter runs the command.
Closing or exiting the visible launcher keeps the game running.

输入 `start` 启动、`restart` 重启、`stop` 或 Ctrl+Z 关闭自有游戏；`cn` 回放中文日志，`help` 查看帮助。
Tab / Shift+Tab 循环补全，Enter 执行。直接关闭或退出可见启动器时游戏继续运行。

`build-info.json` records the source commit, build toolchain and EXE SHA-256/MD5.
The release provides `SHA256SUMS.txt`, `MD5SUMS.txt`, and checksums in its description.
构建信息记录源码提交、工具链及 EXE 哈希；发布页直接展示各附件的 SHA-256、MD5 和中文文件说明，并提供两种校验清单。
