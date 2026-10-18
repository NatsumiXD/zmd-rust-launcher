# Endfield 启动工具（Rust / Windows）

**0.3.1 新增显示语言设置：** 输入 `language` 切换中英文，`language cn` / `language en` 指定语言，均按 Enter 执行并支持 Tab 补全。立即影响后续日志、帮助、状态和输入提示；设置保存在 EXE 旁的 `launcher-settings.json`，下次启动和启动器自更新后沿用，首次默认为英文。`cn` 仅回放中文历史，不改变默认语言。显示语言与游戏参数 `--lang` 独立；保存失败时提示错误并保持原语言，不改官方安装文件。

**当前版本 0.3.1：** 每次成功创建游戏进程后，后台自动检查 Rust 启动器预发布；终端输入 `lud` 可手动再次检查，`update` 仍只更新官方 Games.exe。发现新版后，先并行测速 GitHub 官方、GH-Proxy 与 GHFast，用官方哈希筛选有效通道，再按速度下载、校验并暂存。**只有当前启动器进程退出后才替换 EXE**，不关闭游戏、不自动重开启动器；若提前关闭窗口，后台仍会继续完成下载和更新，完成后自行退出。旧 EXE 和更新日志保留在旁边的 `launcher-update`；失败时保留原文件或尝试恢复备份。详见 [启动器自更新说明](analysis/launcher-self-update.md)。

此前的父进程启动方案已于 2026-10-16 由用户确认进入正常游戏画面，关闭后自有父进程清理完成。游戏检查父进程映像的文件名及签名信息；仅补命令行参数的旧方案会返回认证错误 10100。0.2.0 新功能的验证边界见本文末尾。

0.2.0 已将查询、下载、ZIP 提取、MD5/SHA-256、Windows 签名和版本检查写入 Rust；运行不依赖 PowerShell、Python 或官方安装器。**默认不带参数即按已保存的显示语言输出启动日志、检查更新并启动游戏，创建游戏进程后进入 `endfield> ` 命令终端。** 终端不清屏，保留启动检查、下载、进程创建和错误信息。终端保持响应，游戏继续运行；这不代表已经检测到游戏主界面。

输入命令后按 Enter 执行：`help` 查看帮助，`start`（或 `launch`）启动，`restart` 重启当前会话的游戏，`update` 检查/下载 Games.exe，`preview` 预览，`status` 查看状态，`stop` 关闭自有游戏，`exit` 退出启动器。`restart` 等待旧游戏、挂起父进程和守护清理完成后，按原参数检查更新并启动；当前会话无游戏时直接启动，清理失败则中止重启。**`cn` 一次性显示此前日志的中文翻译，后续日志使用当前默认语言。** 输入时显示候选，Tab / Shift+Tab 向前/向后循环补全，上下键调用输入历史，Backspace 修改输入；旧编号 1/2/3/0 仍可使用。**Ctrl+Z 主动关闭游戏并保留终端**；`exit`、Esc、Ctrl+C 或直接关闭 Rust 窗口只退出可见启动器，游戏继续运行。`--menu` 只打开命令终端，`--launch` 保留无交互终端的命令行启动方式。

运行日志每条按默认语言输出一行（首次默认为英文），包含序号和 INFO/ERROR 级别，不再使用双语分隔块。启动时输出 Endfield.exe 和实际选用的 Games.exe 的完整路径、四段 PE 文件版本；Endfield.exe 的文件版本反映引擎版本，不等同于游戏内容版本。`cn` 按原顺序批量显示当前会话保留的中文日志，不联网翻译、不重复收录翻译内容；历史最多保留 4096 条、4 MiB 文本，超出时提示较早记录已省略。底层系统错误、路径和参数保留原始内容，换行与控制字符会转义。滚轮交由控制台宿主管理；独立传统控制台会扩展至 20000 行缓冲，终端宿主使用自己的滚动历史。

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

## GitHub 自动预发布

[Windows build and prerelease](.github/workflows/prerelease.yml) 在推送 `main` 或手动运行时构建 Windows x64；PR 运行同样的检查和构建但不发布。依次检查格式、Clippy、普通库测试、release 构建、x64 PE 与管理员 manifest，再打包。CI 不启动官方游戏或启动器，依赖版本由 Cargo.lock 锁定。

`main` 上成功构建后创建独立预发布标签 `pre-v<版本>-<运行 ID>.<尝试次数>`，附件包含 EXE、便携 ZIP、`SHA256SUMS.txt`、`MD5SUMS.txt` 和 `build-info.json`；不会设为正式 Latest。发布描述自动生成各附件的中文用途说明，以及全部五个附件的 SHA-256、MD5；两个校验清单各自校验 EXE、ZIP 和构建信息，发布前在云端再次核对。ZIP 仅包含 Rust EXE、使用说明和构建信息，不含官方 Games.exe、游戏文件、缓存或本机证据。每次重新运行使用新标签，已有发布不覆盖。

工作流内置 `GITHUB_TOKEN` 即可发布，只有发布任务申请 `contents: write`，无需额外配置 PAT。可以从仓库 [Actions 页面](https://github.com/NatsumiXD/zmd-rust-launcher/actions/workflows/prerelease.yml) 选择 **Run workflow → main** 手动触发，在 [Releases 页面](https://github.com/NatsumiXD/zmd-rust-launcher/releases) 下载结果。手动选择其他分支只构建，不发布。

打包逻辑为 [Package-Release.ps1](tools/Package-Release.ps1)，只读取已构建 EXE 并校验 manifest，不执行 EXE。发布使用 [GitHub CLI release create](https://cli.github.com/manual/gh_release_create) 的 prerelease 和附件上传功能；构建失败或校验失败不会进入发布任务。

## 分析与证据

Git 仓库保留源码、说明文档和复现脚本；官方二进制、编译产物、下载缓存、原始 REA JSON 和本机测试记录由 `.gitignore` 排除。报告中的原始证据文件链接仅在保留这些文件的研究工作区中可用。

- [最新 Games.exe 的公开接口、提取与验证](analysis/games-acquisition.md)：已验证直接获取官方 ZIP 并只提取签名 Games.exe，无需运行安装器；获取脚本为 [Get-LatestGames.ps1](tools/Get-LatestGames.ps1)。
- [实现原理与完整使用流程](IMPLEMENTATION.zh-CN.md)
- [命令终端、中文日志回放、滚轮修复及验证](analysis/menu-control.md)
- [官方启动器如何更新终末地游戏本体](analysis/endfield-game-update.md)：独立游戏版本接口、分卷下载、校验、补丁调度与尚未恢复的边界。
- [Games.exe 行为与开源替代可行性](analysis/games-open-source-feasibility.md)
- [界面、状态与桥接契约](analysis/games-resources.md)
- [便携目录使用方法](portable/README.md)：已验证复制原版 Games.exe 到工作区后启动成功，仍需该官方签名文件；没有卸载启动器后的干净环境验证。
- [父进程认证与修正方案](analysis/parent-check.md)
- [真实启动验证记录](analysis/runtime-verification.md)
- [最初的启动链分析](analysis/reverse-engineering.md)
- [PE 与历史启动记录](analysis/historical-launch.md)
- [REA 原始证据](rea-evidence.json)，补充聚焦证据位于 `analysis/*-evidence.json`。

自动获取功能此前通过 24 项普通测试和 1 项真实进程探针，并实测 Rust 首次下载、本地复用、缓存无更新和离线模式。当前版本通过 42 项普通测试；工作区控制台探针覆盖命令输入、运行中执行 cn、Ctrl+Z、原生滚轮和模式恢复，退出探针覆盖窗口关闭及强制结束可见启动器后游戏继续运行、守护清理。新版未再次启动真实游戏；此前同一官方 Games.exe 的启动方式已由用户确认进入游戏画面。边界与记录见 [Rust 集成记录](analysis/rust-integration.md) 和 [终端记录](analysis/menu-control.md)。
