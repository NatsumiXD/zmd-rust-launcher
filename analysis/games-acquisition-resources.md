# Games.exe 获取方式：资源与历史更新证据

调查日期：2026-10-16。本调查只读取指定安装目录的程序/资源，以及既有启动器日志中的更新白名单字段；未执行官方程序，未发送网络请求，未修改安装目录，未操作共享 REA native session，未执行任何电源操作。历史日志是过去行为记录，不是本轮运行测试。

## 主要结论

历史记录明确表明：Games.exe 随启动器完整 ZIP 更新包发布。一次 `1.5.0 -> 1.6.0` 更新返回包 URL、包 MD5、包大小、解压总大小；随后 Updater.exe 日志记录下载 HTTP 200、解压、文件检查和复制完成。**不需要从游戏包中提取 Games.exe。** 但尚未凭本报告证明 ZIP 内 Games.exe 的精确成员路径，也没有在本轮下载包进行成员清点。

原生程序的明文字符串包含 `/api/launcher/get_latest`，可以作为后续定位网络请求代码的入口；字符串本身不能证明 HTTP 方法、主机、JSON schema、必填字段或鉴权规则。资源网页也有公开内容接口，但应与原生自更新版本接口区分。

## 身份与证据保存

限定提取脚本：[extract-acquisition-resources.cjs](extract-acquisition-resources.cjs)。保存结果：[games-acquisition-resources-observation.json](games-acquisition-resources-observation.json)。该结果只保留更新元数据、状态、静态字符串与源文件哈希，不保存账号、设备标识、请求关联 ID 或 URL query 的值。

| 文件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `1.6.0/Games.exe` | 17,998,824 | `dab34efc96bf08113fe25a642e38cea05221ae81e8aa31453d5b39add6e2bb55` |
| `1.6.0/Updater.exe` | 1,439,720 | `3119d2409bd0a45a1a78b92ca6bcf1baf08d917ecd8fd16eff7be600e1467744` |
| `1.6.0/Launcher.exe` | 4,464,616 | `0e4dacaaa7022c3b93856f762cae4f7e074ade45f7d096f1eb8f7cb9bc2de2b3` |
| `1.6.0/res/web/index.ff0a17.js` | 998,458 | `b63242eeac361ed930fc3640077afcf04ea56eb5d0161308d3335a62f04cfdfa` |
| `games.log` | 14,284,135 | `f2e2832f73b60ca0b2f8b4031bd9d20dbaff1e7c428dd41df98d28bf351a82ab` |
| `updater.log` | 12,725 | `2d09d3af42726c61a04366c184b5e36b377946a5e41bc0e0a27cc1d71816b2be` |

程序和资源绝对根目录为 `D:\yj\Hypergryph Launcher`。两个日志位于 `C:\Users\miang\AppData\LocalLow\Hypergryph\33a0a6296a20400d503c59ac0fd6341e\logs`。日志行号为从 1 开始的物理行号；静态 native 字符串位置为从 0 开始的文件字节偏移；JS 偏移为 Node 解码 UTF-8 后从 0 开始的字符串字符位置。

## 一次实际历史升级的白名单元数据

`games.log:29235`，日志时间为 `10-11 16:01:58.332`，`OnLastestLauncherInfoChanged` 的 `m_lastestLauncherInfo`：

```json
{
  "action": 1,
  "request_version": "1.5.0",
  "version": "1.6.0",
  "md5": "ac49aa94e1827c44f03c664c5be9c30d",
  "package_size": "132283157",
  "total_size": "405109369"
}
```

同一对象的 `zip_package_url` 去除 query 后为：

```text
https://launcher-sign.hycdn.cn/abYeZZ16BPluCFyT/launcher/1/1/endfield/1.6.0_YMRXC5D5c0y8U0M7/0D1v72GJ/HypergryphLauncher_v1.6.0.1604_1_1_endfield.zip
```

原 URL 包含名为 `auth_key` 的 query 参数，其值未保存。该历史链接不能当作永不过期下载入口；重现获取过程应从版本接口取得服务端当次返回的完整 URL。这里记录 MD5 是恢复原协议字段，不意味着 MD5 能替代 HTTPS 来源校验和下载后对 Games.exe 的 Authenticode 验证。

`games.log:51664`，本地请求版本已经是 `1.6.0` 时，记录为 `action=0`、`request_version=version=1.6.0`、包 URL/MD5 为空、两个 size 为字符串 `"0"`。这支持“同版本时不返回下载包”的该次历史观察；不能据两条样本断言 action 所有枚举值或版本比较算法。

## 历史调用顺序与安装阶段

| 来源 | 限定观察 |
| --- | --- |
| `games.log:29199` | 进入 `Query_ProtocolVer_LauncherVer_GameVer` |
| `games.log:29200` | 进入 `Post_ProtocolVer_LauncherVer_GameVer_Req` |
| `games.log:29201` | `PostClientReq eHttpClientReqType:6` |
| `games.log:29214` | 该 type 6 请求完成，HTTP code 200 |
| `games.log:29217-29218` | `FillData` 与 `ParseLaucherInfoJson` 均记录 parse true |
| `games.log:29235` | 上述启动器升级元数据更新通知 |
| `updater.log:11` | `Download strPackageUrl` 指向相同包路径，记录 bRet true |
| `updater.log:19` | `OnDownloadFinished`：bRet true、nCode 200；缓存文件在旧版本 `1.5.0/updater_tmp` |
| `updater.log:30-40` | 解压开始，完成 nCode 0 |
| `updater.log:59-64` | 文件检查相关完成事件 bRet true |
| `updater.log:93-101` | 复制相关完成事件 bRet true |

这段记录将元数据、下载和安装阶段关联起来，但未确认下载阶段确实逐字节验证了上述包 MD5，也未恢复文件检查清单格式或检查算法。不能以日志显示成功替代本轮对该 ZIP 的独立校验。

### 查询参数的历史白名单

`games.log:179` 的首次历史查询和 `:51629` 的最近历史查询保留了以下非个人字段。这是原生函数日志的参数名，**不是已经证明的 HTTP JSON 键**：

| 原生日志参数 | 早期记录 `:179` | 最近记录 `:51629` |
| --- | --- | --- |
| `strLauncherVersion` | `1.5.0` | `1.6.0` |
| `strLanguage` | `zh-cn` | `zh-cn` |
| `strAppCode` | `abYeZZ16BPluCFyT` | 同左 |
| `strChannel` | `1` | `1` |
| `strGameVersion` | 空 | `1.6.3` |
| `strGameAppCode` | `6LL0KJuqHBVz33WK` | 同左 |
| `strGameChannel` / `strGameSubChannel` | `1` / `1` | 同左 |
| `strProtocolVersion` | 空 | `0` |
| `strTargetApp` | `EndField` | `EndField` |
| `bContainProtocolVersion` / `bContainGameVersion` / `bContainLauncherVersion` | 均 true | 均 true |

`strAppCode` 是启动器发布标识，`strGameAppCode` 是当前游戏发布标识，不能互换。历史升级通知发生时 `games.log:29200` 当前游戏字段是另一个游戏的 `strGameAppCode=GzD1CpaWgmSq1wew`、`strGameVersion=77.0.0`，但启动器自身仍为同一 `strAppCode`、`strTargetApp=EndField`，因此不可只凭当前游戏字段来构造启动器下载 URL。

完整搜索本次 `updater.log` 的 `md5|hash|sha256|signature|signverify|winverify|Games.exe` 未找到匹配行。这里只能说这份 12,725 字节日志没有逐项 Games.exe 签名/哈希记录；**不能据此排除原生代码实际执行校验**。`OnCheckFilesFinished bRet=true` 没有给出算法。Games.exe ASCII/UTF-16LE 的明文 URL 与 domains 限定搜索没有找到原生 launcher base host；网页已知 base host 不能直接填补此缺口。原生 host 仍需追踪其配置加载/解码层。

## 静态接口线索

Games.exe ASCII 字符串：

| 文件字节偏移 | 字符串 |
| ---: | --- |
| 16,532,408 | `/api/launcher/get_latest` |
| 16,532,440 | `/api/launcher/config/main_page/get` |
| 16,532,480 | `/api/launcher/config/protocol/get` |
| 16,532,520 | `/api/proxy/batch_proxy` |

前端 `index.ff0a17.js`：

| JS 字符偏移 | 字符串 |
| ---: | --- |
| 491,293 | `https://launcher.hypergryph.com` |
| 493,573 | `/api/proxy/web/batch_proxy` |
| 496,273 | `version_url` |

前端静态证据 parent `ev_d86c411d956ea0474f4209819408d19905108a8e7bf68314338faeac65ec090e`、summary `ev_d0868cc1452cfa07eb0b4c60d0ff2d228a1f595742c6814fd8fc662cc982e6f9`，保存在 [games-web-rea-summary.json](games-web-rea-summary.json)。JS 图有节点上限，不能宣称全部关系已恢复；`version_url` 在前端用于图像预加载，不能据名称将其当作可执行文件版本清单。

已有原生自更新调度证据 `ev_6e5d10db078a5fcb4415550ceb5657a504d02fa8479a59dcc5545b67348673e9` 对应 `CHgUpdateMgr::_DoHgUpdate`，地址 `0x140102650`，见 [games-selfupdate-code.json](games-selfupdate-code.json)。它支持自更新通过 Updater 层调度，未在先前报告中完整恢复请求协议。

## 供主调查继续确认的具体问题

1. 从 `/api/launcher/get_latest` 字符串 xref 恢复请求主机、HTTP 方法和 JSON 键；历史方法名称有 Post，但仅凭名称不能保证真实 wire method。
2. 确认 `/api/proxy/batch_proxy` 如何封装 get_latest，以及版本查询是否可不依赖登录态。
3. 确认更新版本和发行/渠道字段如何选择；不要把 URL 中 `1/1/endfield` 直接视为完整通用契约。
4. 取得当次公开版本响应后，只有必要时下载工作区文件；先清点 ZIP，再精确提取 Games.exe，不执行安装器。
5. 包哈希与官方签名应分别检查；不得自行生成或复用历史 `auth_key`，也不得将个人令牌/设备信息写进开源默认配置。

本报告未对“当前服务端最新版”作断言：本机版本和历史升级为 `1.6.0`，今天服务端是否已有更新必须另用当次响应确认。
