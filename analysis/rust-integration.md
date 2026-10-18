# Rust 启动器 0.2.0：自动获取与本地优先

当前交互为 `endfield> ` 命令终端：默认输出英文日志并启动游戏，创建游戏进程后接收命令，保留启动日志。`cn` 批量回放此前日志的中文翻译，后续仍输出英文；输入时显示候选，Tab / Shift+Tab 向前/向后循环补全，上下键查历史，Ctrl+Z 关闭自有游戏。当前实现、滚轮修复与新增测试以 [终端记录](menu-control.md) 为准；下文获取功能的历史实测边界保持原记录。源参数/获取逻辑已从 src/main.rs 移至 src/lib.rs，src/main.rs 现在仅为程序入口。库测试不继承管理员 manifest，可在普通终端运行。

当前日志格式为带序号、级别的英文单行；`cn` 使用同序号输出会话中文历史。33 项普通测试、控制台探针、原生滚轮测试及退出保留游戏探针通过，Clippy 无警告；日志与测试记录见终端说明。

当前退出行为：关闭可见 Rust 窗口或退出菜单保留游戏。一个无窗口内部守护保持官方父进程和映像句柄，等游戏结束后清理并退出；Ctrl+Z 仍主动关闭游戏。工作区探针已验证实际窗口关闭、强制终止可见主进程、菜单退出及守护收尾；本轮没有再次启动真实游戏。

日期：2026-10-16。最终程序：`D:\Project\zmd\portable\endfield-direct-launch.exe`，构建副本：`D:\Project\zmd\target\release\endfield-direct-launch.exe`。

SHA-256：`B0390BF14B0F03566E39F46E6DB1A9D4AC4CD68802C412514D023144522C8D41`。两份程序字节一致。

## 最终行为

用户最终选择默认运行时检查/必要下载并启动游戏。后续补充本地 Games.exe 优先复用，已一起实现：

1. 验证游戏路径和启动参数。
2. 检查已有管理缓存，以及工具旁、当前目录、已知官方安装根目录的一层版本目录中的 Games.exe。
3. 校验官方发布者签名并读取 PE FileVersion；取前三段与官方发布版本比较。版本更高的候选优先，同版本先用工具旁/当前目录本地文件，再考虑其他本地目录；不会为了更新覆盖找到的文件。
4. 向官方公开接口查询所选候选的发布版本。版本合适就直接使用，不复制本地文件、不下载 ZIP。
5. 没有候选时用 0.0.0 查询完整包。发现更新时下载官方 ZIP，在独立缓存中检查大小、MD5、唯一 Games.exe entry、官方签名及文件版本，再记录 SHA-256。
6. 验证通过后将新版本目录提交，只原子切换 current.json。旧版缓存保留，失败不切换。
7. 用选中的真实 Games.exe 创建挂起父进程，再启动 Endfield；主线程不恢复，等待游戏退出后清理自有父进程。

该流程仍使用官方签名映像，不是重写官方签名。文件获取现在完全由 Rust 执行；不调用 PowerShell、Python、外部解压器、官方更新器或下载 SDK。

## 使用

在管理员 PowerShell 中执行：

```powershell
# 默认：检查/准备父映像，然后启动游戏
& 'D:\Project\zmd\portable\endfield-direct-launch.exe'

# 指定游戏位置和渲染参数，-- 后可传其他非保留游戏参数
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --game 'E:\Games\Endfield\Endfield.exe' --render dx11 --launch -- '-screen-width' '1920'

# 不启动游戏，只检查/下载 Games.exe
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --update-only

# 不联网、不下载、不创建缓存或进程
& 'D:\Project\zmd\portable\endfield-direct-launch.exe' --dry-run
```

默认缓存是 EXE 旁 endfield-cache；--cache-dir 可指定其他绝对路径。--no-local-parent 跳过本地发现，只使用管理缓存/下载。--offline 明确跳过最新版本检查，使用通过签名验证的本地文件或缓存。--parent 手动指定映像时跳过自动发现和更新，保持旧手动用法。

工具已内嵌 `requireAdministrator` / `uiAccess=false` manifest，双击 EXE 由 Windows 请求管理员权限；从终端调用时使用管理员终端或 `Start-Process -Verb RunAs`。所有模式均使用同一权限声明。参数 --launch、--update-only、--dry-run 互斥。默认游戏语言、子渠道与此前官方记录一致；自动获取固定为本次逆向的 CN/EndField 发布渠道，不能用 --sub-channel 改变下载发行渠道。

## 实现位置

| 文件 | 职责 |
| --- | --- |
| build.rs | 仅为 endfield-direct-launch 二进制设置 MSVC /MANIFEST:EMBED 和 /MANIFESTUAC；进程探针不要求提权 |
| src/main.rs | 默认自动启动、模式/参数校验、有限本地发现、调用准备及启动边界 |
| src/acquisition.rs | 版本选择、收据与哈希、唯一文件解压、有界复制、暂存/提交、管理缓存独占锁 |
| src/native_acquire.rs | Windows WinHTTP、WinVerifyTrust、从已验证 signer 读取发布者、version.dll 读取文件版本 |
| src/windows.rs | 原有挂起父进程和游戏创建/清理；探针新增只读映像句柄兼容验证 |
| portable/Start-Endfield.ps1 | 可选入口，仅向 Rust 传游戏路径和 --launch；已移除跳过自动更新的 --parent |

新增 JSON、ZIP、哈希和 Windows API 绑定依赖由 Cargo.lock 锁定。构建使用 `cargo build --release --locked --bin endfield-direct-launch`；依赖已经取得时可加 --offline。

## 失败与文件边界

更新网络检查失败时，仅允许回退到此前已通过管理缓存验证的映像，并明确提示最新版本未确认。首次只有本地文件但无法联网确认版本时，不静默假定适配；可由用户明确使用 --offline 或 --parent。

收到不符合已知 schema、版本倒退、错误源站、损坏包、错误签名或文件版本不符时停止，不将失败更新设为当前版本。缓存映像每次使用前检查收据 SHA-256、大小、官方签名与文件版本。临时 CDN query 不写入收据。

包大小上限 512 MiB、Games.exe 上限 128 MiB；下载禁用重定向、Cookie 和自动认证。HTTP 设置连接/接收超时和总读取期限。Windows 签名校验不弹 UI，使用系统信任与时间戳策略，不执行在线吊销查询。

输出目录拒绝安装根目录、游戏目录、父目录跳转及重解析点。ZIP 成员路径不作为输出路径，始终只写明确的 Games.exe；目标条目重名或为链接时拒绝。暂存文件失败时只清理本次唯一目录中的已知文件，不递归删除其他目录。包校验至解压、映像签名校验至哈希期间持有只读文件句柄；目录提交后再次固定映像并核对哈希，随后一直保持句柄至游戏生命周期结束。运行中的旧版本目录不被覆盖。

“版本合适”采用官方接口的发布版本粒度，例如 PE 1.6.0.1604 对应发布 1.6.0；接口不提供单文件远程哈希，未声称本地同版本文件与远端 ZIP 逐字节相同。自动查找范围有限，不进行全盘搜索；其他位置可用 --parent 手动指定。

## 验证结果

管理员权限声明补充：当前成品重新通过 release build、fmt 与 Clippy。用 Windows SDK mt.exe 从 EXE 的 RT_MANIFEST/#1 提取并检查了 `requireAdministrator`、`uiAccess=false`，结果保存在 [launcher-admin.manifest.xml](launcher-admin.manifest.xml)。MSVC 链接配置依据 [微软 /MANIFESTUAC 文档](https://learn.microsoft.com/en-us/cpp/build/reference/manifestuac-embeds-uac-information-in-manifest)。此次未再次启动游戏。下面的获取功能运行测试保留原记录。加入 manifest 后初次执行测试曾被错误 740 拦住；菜单改动已将测试移至库，当前普通终端的 26 项测试通过。

- 24 项普通测试通过；其中缓存测试使用明确的模拟网络/签名后端，覆盖首次、无更新、更新切换、坏 MD5、无效签名、错误版本、缓存损坏、离线、旧本地版本及 ZIP/长度边界。另有真实 Windows WinVerifyTrust 拒绝未签名测试 EXE 的检查。
- 单独运行 1 项真实进程探针通过，仅执行工作区自制程序；确认指定父 PID/映像、Unicode 参数、挂起主线程未运行、只读映像句柄兼容以及正常/失败清理。真实控制台事件派发未单独验证。
- Rust 原生首次下载成功：官方 ZIP 132,283,157 字节，MD5 匹配；签名和 PE 版本通过，只缓存 Games.exe，不保留 ZIP。
- 本地优先实测成功：最终便携 EXE 直接使用 `D:\Project\zmd\portable\Games.exe`，没有下载。
- 最终 EXE 管理缓存检查返回无更新，没有重复下载；--offline 没有发起版本查询；--dry-run 未创建不存在的缓存目录。
- fmt、Clippy 和 release build 通过；构建偶有 Windows 增量缓存无法硬链接而自动复制的提示，不影响检查结果。

记录：

| 文件 | 内容 |
| --- | --- |
| rust-integration-tests.txt | 普通测试输出 |
| process-probe-result.txt | 真实自制进程探针与句柄兼容结果 |
| rust-first-download.txt | Rust 首次实际下载记录 |
| rust-final-local-reuse.txt | 最终便携程序使用本地映像 |
| rust-final-no-update.txt | 最终程序缓存无更新 |
| rust-final-offline.txt | 最终程序显式离线 |
| rust-integration-verification.json | 最终 EXE/官方文件哈希与验证边界 |

本轮缓存 Games.exe SHA-256 为 `DAB34EFC96BF08113FE25A642E38CEA05221AE81E8AA31453D5B39ADD6E2BB55`，与此前实机启动所用文件及原安装文件相同。Launcher.exe、Games.exe、Endfield.exe 安装文件哈希均未变化。

**0.2.0 没有再次启动真实游戏。** 此前的同一父进程启动方式已由用户确认进入正常画面；本轮增加功能的验收是原生联网/校验/选择实测、模拟更新测试和自制进程探针，不将其冒称为新的游戏画面验收。未进行卸载官方启动器后的干净系统测试。

未执行休眠。用户“后续不要休眠”的要求继续有效。
