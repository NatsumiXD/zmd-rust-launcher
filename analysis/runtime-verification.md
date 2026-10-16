# 真实启动验证（修正版已通过）

2026-10-15 23:40（Asia/Shanghai），用户允许一次真实启动，并允许游戏正常写日志/状态，仍禁止修改或替换安装程序文件。

- 非管理员调用返回 Windows 740；没有创建游戏进程。
- 随后通过系统 UAC 在管理员上下文运行工作区 Rust EXE。
- 输入为 `-launcher_lang=zh-cn -launcher_sub_channel=1`，工作目录为游戏目录。
- CreateProcessW 成功，返回 PID 25644；见 launch-output.txt 和 launch-result.json。
- 用户报告游戏提示：**“检测到不兼容程序 ida”**。

这表明本次验证遭遇 IDA 兼容性检查。不能把 CreateProcessW 成功当成解决启动器限制，也不能把这次失败归因于模拟参数错误。是否还存在独立的启动器检查，仍待干净环境下验证。

中断恢复后，REA binary_session 报告 `open=false`，当时分析会话已关闭。没有隐藏 IDA、修改反作弊逻辑或向 Endfield 注入任何东西。

## 第二次授权测试

用户随后明确允许再启动一次。23:42:35 以管理员上下文运行 `--launch --wait`，保留 Rust 父进程直到游戏退出，CreateProcessW 返回 PID 21800。游戏显示“游戏启动失败：请通过官方启动器打开游戏”，并于 23:43:07 退出，退出码为 0。这直接证明前述参数/工作目录候选不足；退出码 0 也不代表启动成功。

本次 Player.log 诊断（仅记录启动相关字段）为：

```text
PC launch auth extraInfo failed, code: 1, errorCode: 10100,
filePath: d:\project\zmd\target\release\endfield-direct-launch.exe
PC launch auth failed, errorCode: 10100,
filePath: d:\project\zmd\target\release\endfield-direct-launch.exe
```

调用栈包含 `Beyond.SDK.Impl.U8ExternalTools::_ProcessLaunchAuth`、`U8.SDK.SDKInterfaceWIN::HGU8SDKSetData`、`Beyond.SDK.<WaitIfNeeded>d__11::MoveNext`。这将问题缩小到 SDK 原生启动认证及所报告的启动进程路径，尚不能单凭日志确定完整判定算法。

第二次测试结束后恢复静态分析 U8SDK.dll 和 OneChannelLC.dll，完整检查与修正设计见 parent-check.md；在第三次单独授权之前没有再次执行游戏。

## 第三次授权测试：修正版通过

2026-10-16，用户明确允许验证修正版一次。启动前确认没有 IDA/idalib 工作进程和既有 Games/Endfield 进程。00:13:04 以管理员上下文运行修正版，创建本工具自有的官方挂起 Games.exe（PID 3040），通过父进程属性创建 Endfield（PID 25248）。

用户现场确认：**“出现正常游戏画面后，我关闭了游戏”**。00:13:40 工具等待游戏退出后返回 0，复查 PID 3040 与 25248 均已不存在，挂起父进程自动清理完成。没有执行 Games 主线程、启动器界面或更新逻辑，没有 hook Endfield。

第三次 Player.log 未出现明确的 PC launch auth 成功行，也没有此前的 10100 失败记录；不能仅凭日志缺失证明成功。本次通过依据是用户确认的正常游戏画面，以及进程创建和清理记录。正常游戏画面以后的登录、联网和游戏内容不在本次验证范围。

游戏验收使用的工具 SHA-256 为 `F4ED5BF8C332A702159E5D83F55DE942C7D2C4275E77CA431CBE0A2602673FD7`。验收后仅更新工具的状态提示文字和 Cargo 描述，未改变启动/清理逻辑；最终构建哈希另见 final-verification.json。

安装程序文件的最终只读哈希核对见 final-verification.json。允许的正常游戏日志/状态写入不包含程序文件替换。

## 独立目录测试：用户确认通过

用户随后要求脱离完整启动器测试。工作区 portable 目录仅放 Rust EXE 与原版 Games.exe 两个程序，使用显式 --parent 指向此处 Games.exe。用户手动确认系统 UAC；2026-10-16 00:22:38 至 00:23:09 创建自有父 PID 34872 和游戏 PID 33800，工具返回 0，随后两者均已不存在。用户现场确认“可以启动”。检查时游戏已经关闭，未取得本轮独立窗口截图；成功依据是用户确认和进程生命周期，不能单凭退出码证明。

记录见 portable-launch-output.txt、portable-launch-result.json。工具 SHA-256：`5DF0D08FFF2086182A13568E3059DCF95113E5A2C2B3EC0B2ECA14463D202C11`；复制的 Games.exe 哈希与原版相同。安装目录没有删除、重命名或改权限，未卸载启动器或清理注册信息，未采集全量文件访问轨迹，因此不将本次结果解释为全新系统卸载后验收。
