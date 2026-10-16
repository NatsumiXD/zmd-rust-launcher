# Endfield 启动工具（Rust / Windows）

此前的父进程启动方案已于 2026-10-16 由用户确认进入正常游戏画面，关闭后自有父进程清理完成。游戏检查父进程映像的文件名及签名信息；仅补命令行参数的旧方案会返回认证错误 10100。0.2.0 新功能的验证边界见本文末尾。

0.2.0 已将查询、下载、ZIP 提取、MD5/SHA-256、Windows 签名和版本检查写入 Rust；运行不依赖 PowerShell、Python 或官方安装器。**默认不带参数即输出启动日志、检查更新并启动游戏，创建游戏进程后立即进入输入式菜单。** 菜单不清屏，启动检查、下载、进程创建和错误信息保留在控制台。菜单保持响应，游戏继续运行；这不代表已经检测到游戏主界面。

在启动器窗口输入 **1/2/3/0 后按 Enter** 执行，Backspace 修改输入，**Tab** 切换中英文。**Ctrl+Z 主动关闭游戏并保留菜单**；输入 0 或 Q 后按 Enter，或按 Esc / Ctrl+C，只退出启动器，游戏继续运行。直接关闭 Rust 窗口也保留游戏。空输入和无效编号不会执行操作。游戏自然退出后，菜单更新状态，可再次启动。`--menu` 只打开菜单，`--launch` 保留无菜单的命令行启动方式。

运行日志固定为中文一行、英文一行，中文在上，每条日志上下各有 `================` 分隔线；Tab 只切换菜单语言，不改变日志格式。底层系统错误、路径和参数保留原始内容，其中的换行与控制字符会转义，保持每条日志恰好两行正文。

启动前优先查找本地原版 Games.exe，验证官方签名并读取文件版本，与官方接口版本匹配就直接使用。没有合适本地文件时，从官方完整 ZIP 只提取 Games.exe，写入独立缓存。随后创建它的挂起进程，再通过 Windows 父进程属性启动 Endfield；主线程不会恢复，游戏退出后只清理工具自有父进程。

## 使用

请将本工具保留在安装目录之外。双击 EXE 时，Windows 会根据内嵌的 `requireAdministrator` manifest 请求管理员权限，确认 UAC 后按默认流程检查更新并启动游戏。带参数使用时，可在管理员 PowerShell 中运行：

```powershell
cargo build --release --locked --bin endfield-direct-launch
# 默认：检查更新、准备父映像，然后真实启动游戏
& 'D:\Project\zmd\portable\endfield-direct-launch.exe'
# 同样真实启动，并指定游戏位置
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --game 'E:\Games\Endfield\Endfield.exe' --launch
# 仅检查和下载更新，不启动游戏
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --update-only
# 仅预览：不联网、不创建缓存或进程
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --dry-run
```

工具与游戏均要求管理员权限。普通终端可用 `Start-Process 'D:\Project\zmd\portable\endfield-direct-launch.exe' -Verb RunAs` 请求系统提权；若调用方式不支持 UAC 并返回错误 740，应改用管理员终端或上述命令。`--help`、`--dry-run` 等模式也使用同一权限声明。

Windows 构建使用 MSVC 工具链。实际启动器 EXE 要求管理员权限；逻辑测试已移入库，不需要管理员权限即可运行 `cargo test`。

默认游戏路径为 `D:\yj\Hypergryph Launcher\games\Arknights Endfield\Endfield.exe`。默认游戏参数仍为 `-launcher_lang=zh-cn -launcher_sub_channel=1`；支持 --lang、--render、--sub-channel、--tracking-code 和 `--` 后的其他游戏参数。

本地查找范围为工具旁 Games.exe、当前目录 Games.exe、已知 `D:\yj\Hypergryph Launcher` 的一层版本目录，不递归扫描磁盘。先选较新有效版本；同版本优先工具旁/当前目录的文件。文件必须通过 Windows Authenticode 和官方发布者检查，版本按 PE FileVersion 的前三段与官方发布版本比较，例如 1.6.0.1604 对应 1.6.0。服务没有提供单个 Games.exe 的远程哈希，因此同版本复用不等于已把本地文件与远端 ZIP 逐字节对比。

默认缓存是 EXE 旁 `endfield-cache`，可用 --cache-dir 指定其他绝对路径；拒绝写入上述官方安装根目录或游戏目录。下载校验通过后才原子切换 current.json，旧版目录保留，运行中的父映像不被覆盖。临时 CDN 签名不会写入收据。自动获取固定使用已验证的 CN/EndField 发布渠道，游戏 --sub-channel 参数不会改变下载渠道。

```powershell
# 显式选择原版映像：跳过自动发现及更新，仅供手动控制
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --parent 'E:\Launcher\Games.exe' --launch
# 离线：使用经签名验证的本地文件或缓存，不检查远端最新版本
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --offline
# 跳过本地发现，仅检查/准备管理缓存
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --update-only --no-local-parent
# 查看所有参数
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --help
```

更新检查断网时只允许回退到此前有效管理缓存，并提示“未确认最新版本”；首次只有本地文件而无法联网确认版本时，需用户明确使用 --offline 或 --parent。包损坏、签名/版本不符会停止本次启动，保留此前缓存。签名验证采用 Windows 信任/时间戳策略，不弹 UI，也不执行在线吊销查询。

工具不修改或替换安装文件，不注入 DLL，不 hook 游戏或启动器，也不修改 Endfield 内存。真实启动时游戏自身可能写日志/状态，用户已明确允许这类正常运行写入。

游戏创建后会启动一个无窗口的后台守护进程。可见启动器退出时，守护继续持有原版父进程与映像固定句柄；游戏结束后清理父进程并自动退出，不按进程名批量结束程序。Ctrl+Z 仍通过自有句柄终止游戏。守护使用同一个 Rust EXE 的内部模式，后台会暂时保留一个同名进程。

测试游戏前需正常退出 IDA/REA 分析工作线程：第一次验证曾遇到“检测到不兼容程序 ida”。这属于独立的兼容性提示，本工具不修改该检查。

## 分析与证据

Git 仓库保留源码、说明文档和复现脚本；官方二进制、编译产物、下载缓存、原始 REA JSON 和本机测试记录由 `.gitignore` 排除。报告中的原始证据文件链接仅在保留这些文件的研究工作区中可用。

- [最新 Games.exe 的公开接口、提取与验证](analysis/games-acquisition.md)：已验证直接获取官方 ZIP 并只提取签名 Games.exe，无需运行安装器；获取脚本为 [Get-LatestGames.ps1](tools/Get-LatestGames.ps1)。
- [实现原理与完整使用流程](IMPLEMENTATION.zh-CN.md)
- [输入式菜单、快捷键和本轮验证](analysis/menu-control.md)
- [官方启动器如何更新终末地游戏本体](analysis/endfield-game-update.md)：独立游戏版本接口、分卷下载、校验、补丁调度与尚未恢复的边界。
- [Games.exe 行为与开源替代可行性](analysis/games-open-source-feasibility.md)
- [界面、状态与桥接契约](analysis/games-resources.md)
- [便携目录使用方法](portable/README.md)：已验证复制原版 Games.exe 到工作区后启动成功，仍需该官方签名文件；没有卸载启动器后的干净环境验证。
- [父进程认证与修正方案](analysis/parent-check.md)
- [真实启动验证记录](analysis/runtime-verification.md)
- [最初的启动链分析](analysis/reverse-engineering.md)
- [PE 与历史启动记录](analysis/historical-launch.md)
- [REA 原始证据](rea-evidence.json)，补充聚焦证据位于 `analysis/*-evidence.json`。

自动获取功能此前通过 24 项普通测试和 1 项真实进程探针，并实测 Rust 首次下载、本地复用、缓存无更新和离线模式。当前版本通过 27 项普通测试；工作区进程探针覆盖运行中菜单、Tab 切换、Ctrl+Z 关闭与进程清理，另有双语日志格式和控制字符换行检查。新版未再次启动真实游戏；此前同一官方 Games.exe 的启动方式已由用户确认进入游戏画面。边界与记录见 [Rust 集成记录](analysis/rust-integration.md) 和 [菜单记录](analysis/menu-control.md)。
