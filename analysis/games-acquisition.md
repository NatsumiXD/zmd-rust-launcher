# 最新 Games.exe 的获取方式：逆向结果与验证

后续 0.2.0 已将下述协议写入 Rust 启动器，并增加本地文件优先复用；见 [Rust 集成记录](rust-integration.md)。本文保留最初 PowerShell 协议验证的证据和时间。

记录日期：2026-10-16（Asia/Shanghai）。目标为启动器 `1.6.0\Games.exe`，SHA-256：`DAB34EFC96BF08113FE25A642E38CEA05221AE81E8AA31453D5B39ADD6E2BB55`。

## 结论

**可以从官方公开版本接口取得当次启动器 ZIP 地址，再只提取 Games.exe；获取这个文件不需要运行或安装官方启动器。** 已完成实际下载、包校验、单文件提取和官方签名验证。未执行 ZIP 中的程序、未启动游戏、未替换现有便携包或安装目录文件。

截至 2026-10-16 07:29，本次 CN / EndField 参数组合的接口返回启动器版本 **1.6.0**，提取的 Games.exe 文件版本为 **1.6.0.1604**。这不是所有渠道、地区或以后时刻的统一版本断言。

实现保存在 [tools/Get-LatestGames.ps1](../tools/Get-LatestGames.ps1)。这是验证获取协议的 PowerShell 脚本；现有游戏启动核心仍为 Rust，本轮没有声称已经把联网下载集成进 Rust EXE。

## 1. 版本查询协议

本轮实测成功的公开 GET 请求：

```text
https://launcher.hypergryph.com/api/launcher/get_latest?appcode=abYeZZ16BPluCFyT&channel=1&sub_channel=1&version=0.0.0&target_app=EndField
```

| 参数 | 本次值 | 含义与依据 |
| --- | --- | --- |
| appcode | abYeZZ16BPluCFyT | 启动器发布标识；不是游戏 appcode |
| channel | 1 | 本机 CN 官方渠道记录 |
| sub_channel | 1 | 本次查询子渠道 |
| version | 0.0.0 | 本轮验证可取得完整包的旧版本请求值 |
| target_app | EndField | 本机原生历史请求值，保留原始大小写 |

两次成功探测的对比记录见 [acq-live-probe.json](acq-live-probe.json)：

| 请求 version | HTTP | action | 返回 version | 包字段 |
| --- | --- | --- | --- | --- |
| 1.6.0 | 200 | 0 | 1.6.0 | URL/MD5 为空，大小为 0 |
| 0.0.0 | 200 | 1 | 1.6.0 | 提供完整 ZIP 地址、MD5 与大小 |

因此，查询当前版本可能只得到“无更新”的空包信息；不能把空 URL 解释为获取失败，也不能直接用它下载。`version=0.0.0` 是本轮成功的兼容查询方式，服务未来可能调整，脚本会在不符合已知结构时停止。

响应是**直接的根 JSON 对象**，没有 data/code/msg 包装：

```json
{
  "action": 1,
  "version": "1.6.0",
  "request_version": "0.0.0",
  "zip_package_url": "https://launcher-sign.hycdn.cn/.../HypergryphLauncher_v1.6.0.1604_1_1_endfield.zip?<临时签名>",
  "md5": "ac49aa94e1827c44f03c664c5be9c30d",
  "package_size": "132283157",
  "total_size": "405109369",
  "description": "<版本说明>"
}
```

大小字段在本次响应中是十进制字符串。脚本按整数解析 package_size。没有传入用户令牌、Cookie 或本机设备标识，仍能取得上述元数据和有效下载地址；此结论覆盖本次公开接口调用。

ZIP 地址含临时 auth_key。脚本每次重新查询，只在进程内存中使用完整下载 URL，报告和收据仅保存去掉 query 的地址。不能把旧日志中的完整 URL 作为永久分发地址；本轮未研究签名过期时长，也没有自行生成签名。

## 2. 原生代码如何支持上述结论

本轮 REA/IDA 证据完整导出在 [games-acquisition-native-evidence.json](games-acquisition-native-evidence.json)。地址均针对本机上述 SHA-256 的映像；反编译结果不是原始源码。

| 位置 | 已观察到的内容 | Evidence ID |
| --- | --- | --- |
| 0x1402E6700 | 请求类型 2 映射 `/api/launcher/get_latest`；6/27 映射 `/api/proxy/batch_proxy` | `ev_9b0142217de81e3fcbf9cd32ca099e66903a4a8777bb92597f221ecdb2b73f7e` |
| 0x1402EE760 | `CLauncherHttpClient::SendGetLatestLauncherReq`；可见 appcode、channel、sub_channel、version、target_app 键，以及设备标识请求头的构造 | `ev_b209d3eb2699065a6095505a84a1dfed133dabf05fd7bf946eee099a18d2d0be` |
| 0x1402EC6C0 | 批量请求封装 `kind=get_latest_launcher` 与 `get_latest_launcher_req`，也可包含协议/游戏查询 | `ev_19621e6b97be29507641198094fa66723d8858aaac55b4396daa87a5a2bed9df` |
| 0x1402E75A0 | `_DoClientReq` 把已设置的 server URL 与类型对应路径拼接，再分派 HTTP 请求 | `ev_85aa61f3b918dbfa81abe56ebd4c147f061a55613a5d025b4b67d90b043020c6` |
| 0x1402F42C0 | `CLibNetworkUtils::ParseLatestLauncherJson`；检查并解析上述 8 个响应键 | `ev_5993b31468abe76d9a9a569353a005466ccc0d25f1673bf7ffa9fffb132cc51e` |
| 0x1402E5CE0 | `CLastestLauncherInfo::FillData` 调用解析器，成功后更新 launcher info | `ev_e127af8a9a653305f2c2cd6f44f14ba320c36cd77fb11c87ba83599812ec4444` |
| 0x14029C490 → 0x14029E8D0 | StartUpdate 包装层及实现；保存新版本/包参数，创建工作区更新配置并继续调度 | `ev_d9b5468f9144cab5f48e913573db13679e597102d23afb08b0e2848f44f13a0a`、`ev_2819df3dda7b66aa7dfdb02fe0a2d1950b4e55a7bf9b9c30171d32c79477cbe2` |

接口路径最初还通过原文件偏移 `0xFC43B8` 定位，映射到 VA `0x140FC5DB8` 后用 REA xref 确认。辅助脚本 [locate-pe-text.ps1](locate-pe-text.ps1) 只做离线字节与 PE 节映射，不执行目标代码。

主机 `launcher.hypergryph.com` 最初来自已解析的网页资源，作为候选与原生路径组合后，通过本轮真实 HTTP GET 及下载验证其可用。**本轮未完整恢复原生 server URL 的配置加载链**，不把这个仍有缺口的静态数据来源伪称为已恢复；公开获取方法的可用性由实际响应另外支撑。

历史启动器使用过 batch_proxy 请求并取得同一版本结构；获取单个 Games.exe 所需的包元数据，可以使用已经实测的独立 get_latest 请求，不需要复现所有批量查询、登录或 UI 初始化。

## 3. 实际下载和提取结果

执行时间：2026-10-16 07:29:13 至 07:29:28。完整收据见 [acq-download-verification.json](acq-download-verification.json)。

| 项目 | 实测结果 |
| --- | --- |
| ZIP 字节数 | 132,283,157，与接口一致 |
| ZIP MD5 | ac49aa94e1827c44f03c664c5be9c30d，与接口一致 |
| ZIP SHA-256 | 8F14225E9E708069223663B6A7B039C0CD47F57B40AA7798D05AF5DAF4C2AAD8 |
| ZIP entry 数量 | 247 |
| 提取目标 | 唯一的 `Games.exe` entry；其余条目未解压 |
| Games.exe 字节数 | 17,998,824 |
| Games.exe SHA-256 | DAB34EFC96BF08113FE25A642E38CEA05221AE81E8AA31453D5B39ADD6E2BB55 |
| Authenticode | Valid |
| 发布者 | Shanghai Hypergryph Network Technology Co., Ltd. |
| PE 文件版本 | 1.6.0.1604 |
| 与现有原版文件比较 | SHA-256 相同 |

提取文件保存在：

```text
D:\Project\zmd\artifacts\official-games\1.6.0-0256b8a6952d416fb185f479a592e1f4\Games.exe
```

现有 Rust 工具已用该路径完成 **dry-run**，路径和参数校验通过，没有创建任何游戏/父进程。本轮未进行新的游戏运行验收；它与此前成功启动所用 Games.exe 字节相同，但不能把 dry-run 单独当作运行成功。

MD5 在这里用于与官方元数据保持一致；它不替代 HTTPS 来源限制及提取文件的 Authenticode/发布者检查。证书指纹保存作身份记录，未硬编码为游戏认证规则。

## 4. 可重复执行的命令

只查询，不下载或创建产物目录：

```powershell
& 'D:\Project\zmd\tools\Get-LatestGames.ps1' -MetadataOnly
```

获取当次包并提取文件：

```powershell
$parent = & 'D:\Project\zmd\tools\Get-LatestGames.ps1'
$parent.GamesPath
```

每次下载产生独立目录和 receipt.json；不覆盖此前文件，不替换 portable/Games.exe。默认保留下载 ZIP，便于复查。

将提取结果用于现有 Rust 启动工具：

```powershell
# 当前 Rust 0.2.0 必须显式 --dry-run 才仅预览
& 'D:\Project\zmd\target\release\endfield-direct-launch.exe' --parent $parent.GamesPath --dry-run

# 在管理员 PowerShell 中，用户准备启动时再加 --launch
& 'D:\Project\zmd\target\release\endfield-direct-launch.exe' --parent $parent.GamesPath --launch
```

后一个命令仅为使用说明，本轮没有执行。启动方式仍创建原版 Games.exe 的挂起父进程，不执行其主线程；游戏退出后由 Rust 工具回收。完整流程见 [实现文档](../IMPLEMENTATION.zh-CN.md)。

## 5. 获取脚本的边界和验证

脚本使用官方公开 HTTPS 元数据，不读取安装配置或登录状态。下载仅接受本轮确认的 `launcher-sign.hycdn.cn` 及 CN/EndField 路径，不跟随下载重定向；不匹配当前格式时停止。包限制为 512 MiB，Games.exe entry 限制为 128 MiB。

只在项目 artifacts 下创建唯一新目录；检查输出路径链中的重解析点，并拒绝写入本机官方安装根目录。读取 ZIP 时不把 entry.FullName 当作输出路径，只将唯一目标的内容写入明确的 Games.exe 文件名；不会整体解包到安装目录。

包下载和提取都检查实际长度，校验包 MD5，再验证 Games.exe 的 Authenticode 状态与官方发布者名。失败路径移除本次尚未验证的 Games.exe；不扫描、结束或更改现有程序。临时下载 URL 不进入日志。

已通过脚本语法检查、真实下载/提取/签名验证、Rust dry-run，以及三个有边界流测试：正常字节一致、截断拒绝、超量拒绝。测试记录见 [acq-stream-verification.json](acq-stream-verification.json)。未将“成功处理本版本”扩大为已经验证所有网络故障、未来归档格式或证书轮换。

## 6. 对开源启动工具的意义

自写工具可以在用户需要时查询官方服务并取得必要的原版签名映像，**不再要求用户先安装完整官方启动器或从安装目录手工复制 Games.exe**。发布源代码时也不必附带官方二进制。

这解决的是原版文件的获取与校验，仍然不是用开源代码重新生成官方签名 Games.exe。获取流程可以移植到 Rust：保留相同请求契约、大小/哈希/来源检查和单文件提取，再连接现有 --parent 启动边界。无需调用官方下载 SDK。

此前没有在卸载启动器、清理注册信息后的干净机器上验收游戏；本轮也没有做这项测试。获取脚本不读启动器安装文件，但这不等于已经证明游戏运行不再读取任何既有系统状态。

历史更新/资源证据另见 [games-acquisition-resources.md](games-acquisition-resources.md)。本轮未完整恢复 Updater.exe 的所有包/文件校验与替换算法；报告中的 MD5 和单文件签名校验是获取脚本自行完成的验证，不冒充原版 Updater 全流程复现。

用户已明确要求后续不要休眠；旧休眠辅助脚本已禁用，本轮没有执行电源操作。
