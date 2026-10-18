# 命令终端、中文日志回放与滚轮修复

2026-10-16。默认输出英文日志，检查或下载合适的 Games.exe，创建游戏进程后进入 `endfield> ` 命令终端。屏幕只追加内容，不反复绘制菜单、不清除启动日志。进程创建成功不等同于已经进入游戏画面。

## 使用

双击 `D:\Project\zmd\portable\endfield-direct-launch.exe`，确认 Windows UAC。输入命令后按 Enter 执行，命令不区分大小写。

| 命令或按键 | 行为 |
| --- | --- |
| help / ? | 显示命令说明 |
| start / launch / 1 | 检查 Games.exe 并启动游戏；自有游戏运行时拒绝重复启动 |
| restart | 关闭自有游戏，等待父进程及守护清理后按原参数检查更新并启动；当前会话无游戏时直接启动 |
| update / 2 | 仅检查或下载 Games.exe；自有游戏运行时提示先关闭 |
| lud | 请求后台检查 Rust 预发布、测速、下载和校验；关闭启动器后应用，游戏继续运行 |
| preview / 3 | 预览参数，不联网、不创建进程 |
| status | 显示自有游戏状态 |
| cn | 一次性按原顺序显示此前日志的中文翻译；不改变后续英文输出 |
| stop / Ctrl+Z | 终止自有游戏，保留终端，守护收尾清理挂起父进程 |
| exit / quit / q / 0；Esc / Ctrl+C；关闭窗口 | 退出可见启动器，游戏继续运行 |
| Tab / Shift+Tab | 向前/向后循环补全当前前缀的候选，不执行命令 |
| 上下键 | 调用最近 50 条输入历史，返回末尾时恢复未提交文本 |
| Backspace | 删除一个字符；输入最多 256 个 ASCII 字符 |
| --menu | 仅打开命令终端，不自动启动 |
| --launch | 单次命令行启动并等待，不进入命令终端 |
| --update-only / --dry-run | 保留单次检查或预览行为 |

快捷键仅在启动器控制台获得焦点时有效。空输入只重新显示提示符，无效命令提示输入 help。游戏运行时仍可执行 cn、status、help 和 preview；游戏自然退出后可再次 start。启动检查或下载完成后才接收下一条命令。当前自动更新只涉及 Games.exe，尚未实现游戏本体升级，相关逆向见 [游戏更新报告](endfield-game-update.md)。

2026-10-18 增加 restart；帮助及 Tab 补全同步支持该命令。重启只操作当前启动器会话持有的游戏句柄，不查找或终止其他启动器启动的游戏。旧会话清理失败时记录英文错误，保留终端并中止本次重启；若清理成功但更新或启动失败，记录失败原因并回到提示符。重启日志同样可用 cn 回放。

## 实时候选与 Tab 补全

输入时在同一行右侧显示灰色候选，例如 `endfield> st  [start | status | stop]`。第一次 Tab 填入 start，再按 Tab 依次选 status、stop，随后循环；Shift+Tab 反向循环，当前选择用 `<start>` 标记。候选始终来自开始补全时的前缀，修改输入、退格、切换历史或提交命令后重新匹配。

输入 `re` 后 Tab 补为 restart；字母别名 launch、quit 也可补全。不区分大小写，忽略前导空格。空输入显示命令候选；无匹配时显示英文提示，不修改输入。完整命令显示 Enter to run；候选显示及 Tab 本身均不会执行，Enter 只执行输入区已有的命令。

候选仅重绘当前输入行，不写入运行日志或 cn 历史；提交、快捷键操作、异步游戏退出日志之前先清除候选。光标停在输入末尾、候选之前；颜色随后恢复。窄窗口截短候选，过长输入在当前行显示末尾但保留完整文本，避免自动换行覆盖历史。上下键仍用于命令历史。

## 英文日志与 cn

正常日志立即写入 stdout，错误立即写入 stderr；每条只有一行英文，不再输出双语分隔块。例如：

```text
[0001 INFO] Checking Games.exe and starting the game.
endfield> cn
--- 历史日志中文翻译（1 条）---
[0001 INFO] 正在检查 Games.exe 并启动游戏。
--- 翻译结束；后续日志仍使用英文 ---
endfield>
```

日志调用同时提供中英文消息，cn 使用内存保存的中文消息批量回放，不调用在线翻译接口。编号和级别与英文原记录一致。路径、参数及底层系统错误保留原文，其中换行和控制字符转义，避免破坏终端布局。

启动日志还显示 Endfield.exe 与最终选中的 Games.exe 的完整路径、四段 PE FileVersion；本地复用、下载缓存、离线和手动父映像均读取最终文件。Endfield.exe 在获取父映像前记录，父映像在选定后记录；托管预览不选取父映像，因此不会编造其版本。没有文件版本资源时显示 unknown 及原因，显示失败本身不阻止启动，原有 Games.exe 签名/版本校验仍严格执行。官方更新接口仍使用前三段版本比较，不受日志的四段显示影响。本机读取值为 Games.exe 1.6.0.1604、Endfield.exe 2021.3.34.0；后者是引擎文件版本，不代表游戏内容更新版本。只读预览已与 Windows FileVersionInfo 的四段数值比对，见 `executable-details-probe.txt`；无版本资源的工作区模拟程序也验证了显示失败不阻止预览，见 `executable-details-missing-version-probe.txt`。

历史只包含本次可见启动器进程的运行日志，不含命令回显、帮助或状态界面；不默认写磁盘日志。最多保留 4096 条、4 MiB 中英文文本，超出时淘汰最早记录，cn 显示省略数量。再次 cn 会回放所有仍保留的历史，不重复收录翻译本身；退出后内存历史释放。

## 滚轮原因与修复

旧输入模式关闭了 Quick Edit，却保留 ENABLE_MOUSE_INPUT，ReadConsoleInputW 循环随后消费并忽略 MOUSE_EVENT。鼠标滚动事件因此可能被应用拿走，无法正常用于回看控制台历史。

新输入模式关闭 ENABLE_MOUSE_INPUT，将鼠标和滚动留给控制台宿主，并保留原有 Quick Edit 设置。为捕获 Ctrl+Z、编辑命令而临时关闭行输入、自动回显、处理式输入和虚拟终端输入；作用域退出后恢复原模式。[Microsoft SetConsoleMode 文档](https://learn.microsoft.com/en-us/windows/console/setconsolemode)说明了鼠标输入事件入队和 Quick Edit 标志的语义。

独立传统控制台启动时将屏幕缓冲扩至 20000 行；只有控制台进程列表仅含当前进程时才调整，不修改调用方 PowerShell 的共享控制台。现代终端使用宿主自己的滚动历史，缓冲调整失败不阻止启动。输入期间不清屏、空闲时不重绘整页；Tab 或历史切换仅重绘当前输入行。

原生滚轮测试在工作区程序的私有隐藏传统控制台中写入足够多的行，再向其窗口派发 WM_MOUSEWHEEL：本轮视口顶部由第 45 行变为第 42 行，缓冲为 20000 行，确认一次向上滚动实际生效。证据为 `terminal-scroll-probe.json`；没有据此声称已在所有终端宿主上实测。

## 进程生命周期

可见终端持有 Session 和游戏句柄。游戏创建后，启动同一 Rust EXE 的无窗口内部守护模式，使用 CREATE_NO_WINDOW 和 PROC_THREAD_ATTRIBUTE_HANDLE_LIST 仅继承官方父进程、游戏、只读映像与就绪事件四个句柄。守护验证进程身份和文件类型后确认就绪，主进程才解除自己对挂起父进程的终止责任。

关闭可见窗口、输入 exit、Ctrl+C 或单独强制终止可见启动器后，后台守护继续持有官方父进程与映像固定句柄。游戏结束后守护清理挂起父进程并退出，因此任务管理器中暂时保留一个同名 Rust 后台进程属于预期行为。守护不通过 PID 重新打开目标，不恢复官方主线程。

stop 或 Ctrl+Z 使用自有进程句柄调用 TerminateProcess，属于直接终止游戏；不按名称批量关闭程序，不注入、不 hook、不修改游戏内存。管理员 manifest 仅用于实际启动器，库测试和 process-probe 无需管理员权限。

## 验证与边界

候选补全本轮通过 33 项普通测试、工作区控制台探针、Clippy 和 release 构建。新增用例覆盖多候选循环及反向选择、编辑后重置、别名/大小写/未知前缀/历史交互、单行宽度与长输入。私有控制台实际读取 re → Tab → Enter，并读回控制台缓冲验证实时候选、选中标记、光标位于提示之前、提交前提示清除；既有滚轮、cn、启动/重启和退出保留游戏探针同时通过。记录为 `completion-unit-tests.txt`、`completion-console-tests.txt`、`completion-clippy.txt`、`completion-release-build.txt` 和 `menu-control-probe.txt`。未执行真实游戏。

2026-10-18 重跑 30 项普通测试、控制台探针、Clippy 和 release 构建，均通过。控制台探针新增实际 start → exit、运行中 start → restart → exit、空会话 restart → exit，确认旧游戏/父进程/守护结束、新进程运行、退出保留游戏、无关模拟进程不受影响，最后清理所有自有模拟进程。仅使用工作区的 Endfield.exe/Games.exe 命名模拟程序，无联网或官方程序执行。记录为 `restart-command-probe.txt`、`restart-console-tests.txt`、`restart-unit-tests.txt`、`restart-clippy.txt` 和 `restart-release-build.txt`。

- `cargo test --offline --locked`：30 项通过，5 项探针或辅助角色默认忽略，见 `terminal-unit-tests.txt`。覆盖命令提交/别名/历史、英文单行及控制字符、cn 不改变后续语言、历史上限和控制台模式。
- `cargo test --offline --locked workspace_console_ctrl_c_and_menu_probe -- --ignored --nocapture`：通过。私有控制台实际读取按键，运行中执行 cn → Ctrl+Z → exit；验证启动日志保留、模式恢复、CTRL_C_EVENT 清理及原生滚轮。见 `terminal-console-probe-test.txt`、`menu-control-probe.txt`。
- `cargo test --offline --locked workspace_launcher_close_preserves_game -- --ignored --nocapture`：通过。实际 WM_CLOSE 及强制结束可见测试进程后，模拟游戏、父进程和守护均继续运行；结束模拟游戏后守护清理父进程。见 `terminal-detach-test.txt`、`launcher-detach-probe.json`。
- `cargo clippy --offline --locked --all-targets -- -D warnings` 和 release 构建通过，见 `terminal-clippy.txt`、`terminal-release-build.txt`；最终 EXE 已核验管理员 manifest。

所有运行探针仅执行工作区自制 process-probe，没有启动官方游戏或启动器。此前相同官方父映像的真实启动已由用户确认进入游戏画面；本次不冒称新的真实游戏验收。官方安装文件未修改，未执行休眠。原始本机测试文件由 .gitignore 排除。
