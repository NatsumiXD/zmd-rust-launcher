# REA 调查记录

调查日期：2026-10-15。所有分析输出与代码保存在 `D:\Project\zmd`；REA 使用临时目录中的分析副本。静态阶段未执行目标；随后在用户明确授权下完成两次游戏测试，允许游戏正常写日志/状态。没有修改或替换安装程序文件，没有 hook Endfield.exe。

本文保留最初对进程创建的分析。真实测试证明纯参数候选不足；最新认证判定和修正方案见 [parent-check.md](parent-check.md)，测试经过见 [runtime-verification.md](runtime-verification.md)。

## 目标身份

| 文件 | SHA-256 |
| --- | --- |
| 根 Launcher.exe（与 1.6.0 下副本相同） | `0e4dacaaa7022c3b93856f762cae4f7e074ade45f7d096f1eb8f7cb9bc2de2b3` |
| 1.6.0/Games.exe | `dab34efc96bf08113fe25a642e38cea05221ae81e8aa31453d5b39add6e2bb55` |
| games/Arknights Endfield/Endfield.exe | `09f516d7c5c61933697e974a9c15ce38873fd031ba2a1d1575fdc358c3f046b5` |
| EndfieldBase.dll | `10f0731317b31b94e9843f93ff1744d69aa404f344c0d87d606e6b4fb27e0601` |

## 已观察到的启动链

Launcher.exe WinMain 的反编译结果包含版本目录解析、深链接解析及后续调度。前端资源通过 HGJsBridge 发送 `click_main_btn` 与 `create_game_link` 事件。实际游戏启动逻辑在版本目录 Games.exe 中。

Launcher WinMain：通过 main.cpp 字符串引用 `0x14003C4E7` 定位；证据 `ev_27be629c98b4c4e1e7f902daf1ecb8a8e077d934fcf5fc13e112f6c034d483b1`。反编译不是原始源码，部分函数签名为工具推断。

Games.exe 的相关调用链：

```text
CGame::_StartGame                    0x1400DC290
  CGame::_DoStartGame                0x1400D8DC0
    CGlobalAssist::StartGame         0x1400CA2E0
      CLibGameAssistImpl::StartGame  0x1402522F0
        CGameAssist 包装             0x140264350
          CGameStartUp::StartGame    0x140279FD0
            QtExtThread::StartThread
              CGameStartUp::run     0x14027AED0
```

上述命名来自嵌入日志字符串与调用关系；未命名包装函数的类归属是分析推断。证据分别为 `ev_18cfb47b74cff8300b7ff7584418399f79520aeabcace67ba9e3a041e397c836`、`ev_398004097f72839e41675599f9fd57c55dae7472973d8f8ef266b4d569a80502`、`ev_6523f0aa5445b4729498feb6d4176d1f097ea8475b30651ea148f38a8e78f8e9`、`ev_05b8db6aff35b59a079f3be6341b392675f666c5be3f3d38d804971c8b162996`、`ev_7a243258875e610166e023b1a453eb598c56e248ef4b8d45c80e201a7ea11e8d`、`ev_2ff805f5309a7089a1a8b14567e413271b043d75680efea0dc2a5e268546f4e1`。

## 最终进程创建

`CGameStartUp::run`（证据 `ev_d5a1a2132ae198e6e558ace26712d579eb9bdf8877a21fb3554412e17e20cf0e`）在 `0x14027B371` 调用 CreateProcessW：

- `lpApplicationName`：游戏 EXE 完整路径（对象偏移 +80）。
- `lpCommandLine` 模板：`"%1"  -launcher_lang=%2 %3 %4 %5`。
- `lpCurrentDirectory`：游戏安装目录（对象偏移 +72）。
- 进程/线程 security attributes：NULL；`bInheritHandles=FALSE`；`dwCreationFlags=0`；`lpEnvironment=NULL`（继承启动器环境）。
- STARTUPINFOW：清零后 `cb=104`。

可选拼接：`-force-d3d11`、`-BeyondGameDeepProfiler`、用户设置的附加命令行、`-tracking_code="VALUE"`、`-launcher_sub_channel=VALUE`。`-wid` 出现在另外的平台分支，不能据此将其解释为通用启动认证标记。

同一 run 函数中未观察到注入 DLL、自定义环境块或继承句柄。这个结论只覆盖所检查的函数，**不能排除其他线程或前序初始化创建的状态**。

Rust 初版选择只复现这个进程创建边界，没有启动官方 Launcher/Games 进程或 patch 二进制。默认语言/子渠道由现有历史记录确认，见 historical-launch.md。该版本在真实测试中返回 SDK 认证 10100，现已沿该错误定位到真实父进程文件检查；修正方案见 parent-check.md。

Endfield.exe manifest 明确要求 `requireAdministrator`，见 `ev_498daa92b088718ee3d391e86c45b3ee070dcd4e18384adf56ce87f29d2e001c`。独立工具不自提权，调用方应通过管理员终端运行；错误 740 不等同于游戏的“必须通过启动器启动”检查。

## 调查覆盖与限制

1. **拒绝条件：已定位本次分支。** 最初对受保护入口的搜索没有定位提示；后续日志、U8SDK.dll 和 OneChannelLC.dll 分析确认本次 10100 是父进程文件名未匹配。其他认证分支的覆盖见 parent-check.md。
2. **端到端验证：修正版已通过。** 前两次授权测试分别遇到 IDA 兼容性提示和“请通过官方启动器打开游戏”；2026-10-16 第三次单独授权的测试中，用户确认修正版进入正常游戏画面，随后关闭，自有父进程已自动清理。
3. **受保护入口反编译：失败。** Endfield.exe 入口地址查询返回输入无效；EndfieldBase.dll 导出 `0x180251600` 的反编译长时间无结果，后续 binary_session 显示 `ProviderAdapterError: Provider ida adapter failed during procedure_pseudo_code`。没有重装或修改 IDA/REA 配置。
4. **证据覆盖：有边界。** IDA 适配器不提供完整 typed references、函数 body ranges 等部分信息。Games.exe 的全量 search_strings 被上游截断，保留的 partial observation 不是完整字符串搜索结果；关键结论均另有聚焦 xref/反编译支撑。

## 验收标准

编译、参数构造测试与 dry-run 只能验证工具实现。本次修正方案在退出分析工具后真实启动，用户确认到达正常游戏画面；验收边界为本机当前安装版本的启动阶段。该方案创建官方挂起父进程，但不执行启动器主线程、界面或更新逻辑，不能声称完全没有官方进程。
