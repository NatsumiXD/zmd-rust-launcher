# 便携启动包

当前 EXE 为 0.2.0：默认输出启动日志、检查更新后启动游戏，创建游戏进程后进入输入式菜单，游戏继续运行。菜单不清屏，启动日志保留在控制台。查询、下载、解包和验证均在 Rust 内完成，不依赖此目录的 PowerShell 脚本。EXE 已内嵌管理员权限声明，双击后确认 Windows UAC 即可运行。

在启动器窗口输入 1/2/3/0 后按 Enter 执行，Backspace 修改输入，Tab 切换中英文。Ctrl+Z 主动关闭游戏并留在菜单。输入 0 后按 Enter，按 Esc / Ctrl+C 或直接关闭窗口，只退出启动器，游戏继续运行。`--menu` 只打开菜单；`--launch` 和下列 PowerShell 脚本仍使用无菜单启动方式。

日志固定同时输出中文和英文，中文在上，每条日志上下使用 `================` 分隔；菜单语言切换不影响日志。

已于 2026-10-16 验证：本目录只有 Rust 工具和未改动的 Games.exe 两个程序时，可以启动游戏；用户确认“可以启动”。随后已自动清理自有挂起父进程。

在管理员 PowerShell 中运行：

```powershell
& 'D:\Project\zmd\portable\Start-Endfield.ps1'
```

游戏位置不同时：

```powershell
& 'D:\Project\zmd\portable\Start-Endfield.ps1' -GamePath 'E:\Games\Endfield\Endfield.exe'
```

推荐直接在管理员 PowerShell 中运行 Rust 工具：

```powershell
& 'D:\Project\zmd\portable\endfield-direct-launch.exe'
# 仅检查/获取文件
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --update-only
# 仅预览，不联网或启动
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --dry-run
```

旁边的 Games.exe 签名及版本合适时会直接使用。文件缺少或版本落后时，Rust 从官方 ZIP 提取新文件到旁边的 endfield-cache；不会覆盖本目录已有 Games.exe。Start-Endfield.ps1 已改为调用同一自动流程，仅负责传递游戏路径，不负责下载或提权。--parent 显式指定路径会跳过自动检查。

本包无需复制完整启动器的 Qt DLL、网页和更新组件，但仍依赖未改动、保留原签名和文件名的 Games.exe。测试时没有卸载原启动器，也没有修改其安装目录或注册信息，因此不声称已验证全新系统的卸载后环境。游戏文件本身仍需保留。

官方父进程主线程保持挂起，不执行启动器界面。Rust 主窗口可以提前关闭；同一 EXE 的无窗口后台守护会保持父进程，等游戏结束后清理并自行退出。
