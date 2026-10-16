# 安装文件与历史启动记录的只读调查

## 调查范围与操作边界

本记录来自文件清单、PE 结构解析、静态字符串搜索和既有日志的限定字段提取。未运行 `Launcher.exe`、`Games.exe`、`Endfield.exe` 或其关联程序，未修改两个安装目录中的文件，未读取账号数据库，未记录账号、令牌或其他身份字段。

检查范围：

- `D:\yj\Hypergryph Launcher` 根目录、`1.6.0` 版本目录及启动器 Web 资源。
- `D:\yj\Hypergryph Launcher\games\Arknights Endfield` 的程序文件、相邻 DLL、目录清单与日志目录。
- 用户 `AppData\Local`、`LocalLow`、`Roaming` 中名称匹配 Hypergryph/GRYPH/HG/Endfield 的顶层目录；实际找到 Hypergryph 的 LocalLow 与 Roaming 目录。后续仅定位日志，不读取账号配置。
- 适用父目录 `D:\AGENTS.md`、`D:\Project\AGENTS.md`、`D:\Project\zmd\AGENTS.md` 均不存在。

根目录 `Launcher.exe` 与 `1.6.0\Launcher.exe` 的大小均为 4,464,616 字节、版本均为 `1.6.0.1604`，SHA256 相同：`0E4DACAAA7022C3B93856F762CAE4F7E074ADE45F7D096F1EB8F7CB9BC2DE2B3`。`1.6.0\Games.exe` 是另一个 17,998,824 字节的程序，包含游戏启动相关静态字符串。

## Endfield.exe 的 PE 结构

`Endfield.exe` 为 x64 PE，文件大小 827,960 字节，版本资源为 `2021.3.34.0`。版本资源本身不能证明入口实现是未经修改的 Unity stub。

| 项目 | 值 |
| --- | --- |
| ImageBase | `0x140000000` |
| AddressOfEntryPoint RVA | `0xA9F0B` |
| 入口 VA | `0x1400A9F0B` |
| 入口文件偏移 | `0xA470B` |
| Import Directory RVA | `0xA9EBD` |
| 唯一静态导入 | `EndfieldBase.dll`，ordinal 1 |
| 该导入的 IAT VA | `0x1400A9EAD` |

九个节的名称均为 `.std`：

| RVA | VirtualSize | 文件偏移 | RawSize |
| --- | --- | --- | --- |
| `0x1000` | `0xC3D0` | `0x400` | `0xC400` |
| `0xE000` | `0x98FA` | `0xC800` | `0x9A00` |
| `0x18000` | `0x1C78` | `0x16200` | `0x200` |
| `0x1A000` | `0xE94` | `0x16400` | `0x1000` |
| `0x1B000` | `0x100` | `0x17400` | `0x200` |
| `0x1C000` | `0x8A158` | `0x17600` | `0x8A200` |
| `0xA7000` | `0x233B7` | `0xA1800` | `0x23400` |
| `0xCB000` | `0x87` | `0xC4C00` | `0x200` |
| `0xCC000` | `0xAA` | `0xC4E00` | `0x200` |

这是非常规的节布局；本调查没有据此认定具体保护方案或品牌。

## EndfieldBase.dll 的入口和导出

| 项目 | 值 |
| --- | --- |
| ImageBase | `0x180000000` |
| DLL 入口 RVA / VA | `0x22AF90` / `0x18022AF90` |
| DLL 入口文件偏移 | `0x21B390` |
| Export Directory RVA / Size | `0x6DDFB0` / `0x50` |
| 唯一导出 | ordinal 1，无名称 |
| ordinal 1 RVA / VA | `0x251600` / `0x180251600` |
| ordinal 1 文件偏移 | `0x241A00` |

该导出是 `Endfield.exe` 唯一静态导入所引用的函数。仅有导入表证据，尚不能据此解释它的实际运行行为。

## 历史启动日志的白名单结果

日志路径：`C:\Users\miang\AppData\LocalLow\Hypergryph\33a0a6296a20400d503c59ac0fd6341e\logs\games.log`。只筛选含 `cgsu create game process` 的行，并提取程序路径、下列白名单参数及创建结果；没有复制完整日志行。

Endfield 的匹配行号为 **49043、49204、49335、50301、50496**。五次记录一致：

| 字段 | 结果 |
| --- | --- |
| 程序路径 | `D:\yj\Hypergryph Launcher\games\Arknights Endfield\Endfield.exe` |
| launcher_lang | `zh-cn` |
| launcher_sub_channel | `1` |
| force-d3d11 | 不存在 |
| tracking_code | 不存在；不提取或记录其值 |
| bRet | `1` |
| 工作目录 | 对应行没有记录，无法确认 |

可确认的共同白名单参数为 `-launcher_lang=zh-cn -launcher_sub_channel=1`。这不声明完整命令行只有这两个参数。`bRet:1` 只表示启动器报告进程创建成功，**不能证明游戏随后通过启动检查、认证或进入游戏**。同目录 `launcher.log` 没有匹配的 `cgsu create game process` 事件。

## 静态字符串证据与限制

`Games.exe` 的启动相关字符串包括：

| 字符串 | VA |
| --- | --- |
| `CGameStartUp::run` | `0x140FB1738` |
| `%1 -wid=%2` | `0x140FB1530` |
| ` -tracking_code="%1"` | `0x140FB1668` |
| ` -launcher_sub_channel=%1` | `0x140FB1680` |
| `"%1"  -launcher_lang=%2 %3 %4 %5` | `0x140FB16A0` |
| `QtExtProcess::_DoCreateProcess` | `0x140FD1C58` |

字符串模板不等于实际执行路径，也不能单凭 `-wid` 的名字判断其意义。启动器静态 Web 资源通过 `QWebChannel` 的 `HGJsBridge.SendMsgToNative` 发送 `click_main_btn` 和 `create_game_link`，数据结构包含游戏 UUID；这说明存在官方启动/快捷方式处理入口，不能替代对原生处理器的分析。

对 `Endfield.exe`、`EndfieldBase.dll`、`UnityPlayer.dll` 搜索了 ASCII 与 UTF16 字符串中的 launcher/from_launcher/launch token/CommandLine/请从/启动器。前两个文件没有命中；UnityPlayer.dll 仅命中通用 Unity 命令行说明与 Win32 API 名。

对 320,407,528 字节的 `GameAssembly.dll` 使用分块搜索，检查 UTF8/ASCII 与 UTF16LE 的 launcher_lang、launcher_sub_channel、from_launcher、from launcher、启动器、请从、`-wid=`、endfield 等已知词，均未命中。仅发现通用环境变量、文件映射 API 与 Unity Burst 共享内存相关名称。`Games.exe` 的部分文件映射日志属于 MMKV，部分 IPC 日志属于补丁流程；未证明它们与 Endfield 启动检查有关。

**未证据化的事项：** 是否检查父进程、是否依赖认证数据或运行时握手、是否读取特定环境变量/共享内存、是否还有其他启动条件，以及报错的具体调用路径。静态未命中不能排除运行时解码、动态构造、资源内字符串或其他模块中的实现；日志中的参数相关性也不能证明它们足以通过检查。
