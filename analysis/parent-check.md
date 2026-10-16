# 已定位的启动认证

## 结论

游戏内的 U8SDK.dll 动态加载 OneChannelLC.dll，后者根据当前游戏的父进程映像文件名和签名信息进行认证。第二次测试返回的 10100 对应未匹配允许文件名的分支；普通 Rust 父进程无法仅靠命令行参数满足它。

这是启动器认证，与第一次检测到 IDA 的兼容性提示分别记录；没有修改反作弊逻辑或隐藏分析工具。

## 调用证据

`U8SDK.dll!HGU8SDKSetData`（`0x180039010`）的 case 2003 调用 `U8SDKImpl::U8StartLaunchCheck`（`0x18005F530`）。证据：`ev_d8df9d4818d22d3eda3789897b664c399052f974e16d1cefd29201ae80b8fe8d`、`ev_191d6388b690713f889a9be9b9b5d63ff7d1d30570467c09fdb672a479c33723`。

该函数解码16字符 DLL 名称，完成文件信任校验后 LoadLibraryW，再调用 ordinal 1 和 ordinal 2。对名称解码器 `0x1800704D0` 的返回算法和调用点立即数做离线重放，得到 `OneChannelLC.dll`；证据 `ev_c285699c2af47e02b47b9b27c61a4506bcedf1eea8ac3a1a105ef83b0c2596fe`。离线重放不执行目标 DLL。

OneChannelLC.dll ordinal 1 在 `0x18002D9D0`，解析启动认证配置后调用 `0x1800144D0`。证据 `ev_ac6b6e68c3b6238bb3088b4944475349f66303f5bc6093a7de70f04022595670`、`ev_067bce76b3507c6fdbdfea36aa71eb7414cf56957eb72605d42d60926e3ecf85`。

## 判定流程

1. `getpid()` 获取游戏 PID，`0x180008C70` 用 Toolhelp32Snapshot + Process32FirstW/NextW 查 `th32ParentProcessID`。
2. `0x180008D00` 用 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` 和 `QueryFullProcessImageNameW` 获取父映像路径，并转小写。没有通过模块列表读取父进程路径。
3. 提取父映像 basename，与认证配置允许的文件名列表比较。遍历后未匹配时返回 **10100**，同时把父映像路径放入结果。
4. 匹配后 `0x180013AA0` 走文件信任校验；失败返回 **10101**。可见 GUID 为 Windows 通用 Authenticode 验证操作。
5. `0x1800140C0` 获取签名信息，后续与配置中的字符串列表比较；不匹配返回 **10102**。配置的完整内容与所有其他认证分支没有全部恢复，不能将其简化为仅比较进程名。
6. 通过分支设置成功及错误码 0。

相关辅助函数证据：父 PID `ev_8f068a162a779968418959a072499649122bfe5d957632400c119a84bfc2edbd`；父映像查询 `ev_12ccc7a7eb0322caa0cb718b6ff762535631a87789abafe83828ab4055e15e80`；文件信任 `ev_1a2f1b7d61e7ead28e1de7c2bf033fb419b700ec04a2172b526dddc34b281c50`；签名信息读取 `ev_0baaeb00d8e1a03ae504a37c4975e8f6d6b8200e24b6b7eac8d245e60bfaa6bd`。

上述 10100/10101/10102 是 OneChannelLC 判定函数中的分支；U8SDK 自身还有同号错误分支，因此只对本次 Player.log 与实际调用路径合并解读。

## Rust 修正方案

使用安装目录内原封不动的官方 `1.6.0\Games.exe` 映像创建挂起进程，将它的 handle 通过 `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` 传给游戏进程创建调用。游戏查到的是该官方父映像的真实路径和磁盘签名。启动器主线程保持挂起，因此不执行其界面和更新逻辑；游戏运行过程中保持父进程存在，游戏退出后仅清理本工具刚创建的父进程。

该方案不修改文件、不注入 DLL、不 hook Endfield.exe，也不更改它的内存。它仍然创建一个官方 Games.exe 进程，并不等于系统中完全没有启动器映像。

Win32 依据：[UpdateProcThreadAttribute](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)、[STARTUPINFOEXW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-startupinfoexw)、[进程创建标志](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)。指定父 handle 需要 PROCESS_CREATE_PROCESS，属性值必须存活到属性列表销毁；此方式会影响文档所述的父属性继承。

工作区自制进程探针已验证：子进程读取到指定父 PID 和父映像路径；父 main 没有执行；正常退出、创建失败和直接调用清理回调均回收了自有父进程，无关探针进程保留。记录见 process-probe-result.txt。最初控制台子系统探针遇到 0xC0000142，改用与真实 Games/Endfield 一致的 GUI 子系统后通过；生产创建标志未为探针改动。实际控制台事件派发与操作系统强杀清理未验证。

13 项普通测试、显式运行的 1 项工作区实际进程探针、fmt/check/release build 及默认 dry-run 均通过。修正版游戏验收 EXE SHA-256：`F4ED5BF8C332A702159E5D83F55DE942C7D2C4275E77CA431CBE0A2602673FD7`。

2026-10-16 第三次单独授权的测试中，用户确认进入正常游戏画面，随后主动关闭；工具已自动清理自有挂起父进程，修正方案通过本机当前安装版本的验收。详细证据及日志边界见 runtime-verification.md。

## REA 运行环境限制

中断恢复后的 MCP 原生会话出现 cleanup_incomplete 并等待旧工作线程，故使用同一 REA 6.5.0 CLI、同一现有 IDA 配置继续只读调查，没有修改配置或安装引擎。CLI 的部分操作在关闭阶段也返回 cleanup_incomplete，但其 `details.partial_observation` 包含前后身份核对完成的反编译结果；这里保留完整 JSON 并明确关闭失败，不伪称完整成功。

调查中还发现 23:30:49 创建的一组 REA idalib 后台进程残留；已按 PID、创建时间与命令行核对后仅结束这些本任务分析进程，没有按 IDA 名称关闭用户窗口。后续真实测试应复查 idalib/Python 工作进程，不能只检查 ida.exe。
