# Games.exe 资源侧与替代实现边界

调查时间：2026-10-16。只读检查已安装文件和已有分析证据，没有执行官方程序、发网络请求或改动安装目录；本调查没有打开或关闭共享 native session，没有读取账户配置值。

## 结论

现有资源足以恢复启动器前端与原生程序之间的一部分数据契约，可据此设计开源的游戏列表、设置、启动、进度显示界面。下载清单协议、补丁格式和完整状态转换仍需原生函数证据；只凭资源不能宣称可以完整替代 Games.exe。此前游戏端检查的是启动父进程文件与官方签名，功能重写不能自动取得这份签名。因此，当前“开源启动前端 + 用户本机提供的原始 Games.exe 挂起父进程”和“完全不需要官方二进制的替代品”是两个不同的目标；已有证据只支持前者。

## 文件身份和观察范围

目标资源目录：`D:\yj\Hypergryph Launcher\1.6.0\res\web`。下述 `index.ff0a17.js` 均指该目录文件，源码被压成一行；位置用 **JavaScript 字符串从 0 开始的字符偏移**，不是 PE 地址或 UTF-8 字节偏移。偏移由 Node `readFileSync(..., 'utf8')` 后 `indexOf`/正则定位，资源代码没有执行。

| 文件 | SHA-256 |
| --- | --- |
| `index.ff0a17.js` | `B63242EEAC361ED930FC3640077AFCF04EA56EB5D0161308D3335A62F04CFDFA` |
| `index.html` | `6A39E9DD0B16B9B1F965C6F6151871EFAC311EE66793978861382BFCEF1A4C96` |
| `version.json` | `4DC6F4FBAB6CABD8D77D9620D8DC3B0D78EE32CC63A32D9250B027F821412064` |

`version.json:2` 的前端版本为 `0.2.15`，文件清单包含主 JS、25 个小 JS chunk、CSS、HTML 和一张图；`env=prod`、`region=cn`。这是 Web 资源版本，不是 Games.exe 或游戏客户端版本。

REA 针对主 JS 的静态解析证据：parent `ev_d86c411d956ea0474f4209819408d19905108a8e7bf68314338faeac65ec090e`，summary `ev_d0868cc1452cfa07eb0b4c60d0ff2d228a1f595742c6814fd8fc662cc982e6f9`，保存在 [games-web-rea-summary.json](games-web-rea-summary.json)。解析了 998,458 字节、1 个 JS、183,558 个 AST 节点，未发生解析错误；语义图触及 20,000 节点上限，其他 27 个已清点资源未纳入该次解析。不能把图中未恢复的关系解释为不存在。

版本目录还存在 Qt5Core/Gui/Network/Widgets/WebEngine/WebChannel、`hgdownloadsdk.dll`、`libcurl.dll`、`7z.dll`、`7zg.exe`、`Patch.exe`、`Updater.exe`、`sqlite3.dll`。这些只是打包文件观察，不能单凭存在就判定 Games.exe 实际调用或调用顺序。

## UI 与原生桥接契约：静态可确认

`index.html:1` 通过 `qrc:///web/js/qwebchannel.js` 加载 Qt WebChannel；主 JS 约字符 `313789` 初始化 `new QWebChannel(qt.webChannelTransport, ...)`，取 `objects.HGJsBridge`。

- 网页向原生：`SendMsgToNative(event, JSON.stringify(data))`；桥接初始化还发送 `init` 与空字符串。
- 原生向网页：订阅 `SendMsgToWeb`，将第二个参数 JSON 解析后，用第一个参数作为事件名分发。JSON 解析失败返回空对象。
- 需要响应的请求包装为 `{data: ..., callbackEvent: ...}`，网页先注册一次响应监听，超时 10 秒后拒绝；静态未看到请求关联 ID，不能据此保证同名并发请求的匹配语义。
- 页面完成初始化后发送 `pageLoaded`（字符 `539204`）。

这些是前端代码中的调用/订阅结构，尚未通过这次调查确认对应原生接收器均实现、运行可达或与 Games.exe 版本严格一致。

| 请求 / 操作 | 前端可见数据 | 响应或通知 | 定位字符偏移 |
| --- | --- | --- | --- |
| `downloaded_game_list_req` | 未提供业务 data | `downloaded_game_list_rsp`，读取 `items`，每项有 `uuid` | `527436` |
| `all_game_list_req` | 未提供业务 data | `all_game_list_rsp`，交由列表处理器 | `527693` |
| `switch_game_req` | `{from: uuid, to: uuid}` | `switch_game_rsp`，读取 `result`、`cur_game`、`show_404_page` | `526286`、`526598` |
| `game_list_update_req` | `{gameList: items}`，来自列表拖动排序 | `game_list_update_rsp`，可回传 `items` | `528828` |
| `main_btn_state_req` | `{uuid}` | `main_btn_state_rsp` / `main_btn_state_notify` | `939094`、`312422` |
| `click_main_btn` | `{uuid}` | 未在该调用处等待响应 | `943245` |
| `set_game_setting` | `{uuid, render_mode: "dx" 或 "vk"}` | `game_setting_notify`：读取 `uuid`、`show_render_mode`、`render_mode` | `942177`、`941392` 附近 |
| `create_game_link` | `{uuid}` | 未在该调用处等待响应 | `945511` |
| `check_game_complete` | `{uuid}` | 未在该调用处等待响应 | `945627` |
| `clear_download_cache` | `{uuid}` | 未在该调用处等待响应 | `945763` |
| `pre_download_btn_state_req` | `{uuid}` | `pre_download_btn_state_rsp` / `pre_download_btn_state_notify` | `952048` |
| `click_pre_download_btn` | `{uuid}` | 未在该调用处等待响应 | `952870` |
| `open_dialog` | `{dialog_name: "game_setting" 或 "launcher_setting", uuid}` | 原生对话框细节未知 | `945370`、`536197` |

`game_list_update_req` 是 UI 排序事件，不能误解为检查游戏新版本。`click_main_btn` 是按当前状态分派的按钮意图，不是单一“启动”指令；不能直接把每次点击映射成 CreateProcessW。

桥接还出现窗口最小化、关闭、Toast、日志和网页 URL 打开请求。与账户有关的事件/代码存在，但本报告不读取用户存储，不恢复认证数据，也不把账户流程纳入可替代范围。

## 状态和数据模型

主 JS 字符 `399700` 附近的主按钮状态枚举包含：

```text
continue_download, download_game, downloading, checking_download_zip,
checking_game_complete, installing, download_except, download_paused,
queuing_download, start_game, update_game, game_service_stopped,
in_game, need_login, need_purchase, prepare_start_game, unbook,
booked, disable_download
```

预下载按钮枚举额外包括 `none`、`pre_download_finish`；其他项覆盖下载、暂停、排队、异常、校验等状态。资源只给出状态词及 UI 分支，不给出完整转换图和持久化规则。

游戏项目在主按钮组件中至少读取 `{uuid, game_state, download_state, pre_download}`；`download_state` 至少读取 `{step, progress}`，其中 `progress={from,to,value}`。进度显示按 `value/(to-from)` 计算，并排除 NaN。任务步骤枚举为 `download`、`check`、`decompress`、`copy`、`game_complete_check`（字符 `401100` 左右；消费处 `937350` 附近）。

按钮状态响应至少读取 `{state,text_normal,text_hover,tooltip,sub_tip}`（字符 `941392` 附近）。这些字段可以成为新实现的参考 DTO，但不能直接当成经过完整验证的接口 schema。

页面初始化参数来自 URL query（字符 `538494` 附近）：`uuid`、`app_code`、`channel`、`sub_channel`、`region`、`theme`、`is_game`、`show_404_page`、`lang`、`main_btn_width`、`launcher_channel_name`、`launcher_region`、`device_id`、`all_game_uuid`、`all_game_first_theme`。默认语言 `zh-cn`；CN 默认 channel/sub_channel 为字符串 `1`，SG 默认 `6`。这是前端初始化默认值，不能替代实际游戏下载配置或游戏启动渠道验证。

## 内容服务与更新协议边界

字符 `491022` 的 `lm` 包装 `fetch`：基础地址字面量为 `https://launcher.hypergryph.com`，通常 POST JSON；GET/HEAD 把参数写入 URLSearchParams。HTTP 不成功或响应含非零 `code` 时抛错。

字符 `493569` 的批量内容端点为 `/api/proxy/web/batch_proxy`，请求 `proxy_reqs` 中有 `get_sidebar`、`get_single_ent`、`get_main_bg_image`、`get_banner`、`get_announcement`、`get_url_config`；公共请求字段在字符 `494000` 附近包括 `appcode`、`language`、`channel`、`sub_channel`、`platform: "Windows"`、`source: "launcher"`。代码支持可选的用户令牌/设备标识请求头；本次没有读取任何实际值或发出请求。

这说明公告/背景/链接等页面内容可以与下载器分开设计；它**不是下载包或增量补丁 API 的证据**。`get_single_ent` 返回值被读取 `version_url`/`button_url` 并用于图像预加载，名字含 version 不足以认定为客户端版本清单。

未恢复：包 URL/分片/哈希类型、包签名、版本比较、断点续传格式、差分补丁应用、原子替换/失败回滚、自更新进程间协议。按“不修改安装目录”的当前约束，只能继续做只读清单分析和工作区模拟，不能真实验收下载/修复/升级写入游戏目录。

## 本地配置和数据库的限定观察

- `1.6.0\res\config\app.data`：48 字节，头部不是明文 JSON，未解码。
- `Cache\Config\Config` 和 `.crc`：均 4,096 字节。未将其当成可公开配置契约，未读取其中可能的个人状态。
- `1.6.0\eld_Games.db`（106,496 字节）和 `eld_Updater.db`（8,192 字节）：REA `inspect_sqlite_database(schema_scope="database")` 均拒绝为 `file is not a database`。未拿到 schema 或 rows，不能声称它们是普通 SQLite，也不能仅据错误断定它们一定加密或确定算法。未尝试解密或读取账户表。

## 对开源替代的具体含义

可从本报告规划自有数据模型、界面和任务状态机，不需要照搬 Qt 资源或官方前端代码。对于仅启动已安装 Endfield 的最小目标，先前报告已有完整进程创建边界：绝对游戏路径、游戏目录 cwd、语言/子渠道参数；见 [reverse-engineering.md](reverse-engineering.md)，证据 `ev_d5a1a2132ae198e6e558ace26712d579eb9bdf8877a21fb3554412e17e20cf0e`。

然而，独立实现的 Games.exe 即使改成同名，也不会因此获得官方 Authenticode 签名。游戏侧的父进程路径、文件名与签名检查见 [parent-check.md](parent-check.md)，`OneChannelLC.dll` 检查证据 `ev_067bce76b3507c6fdbdfea36aa71eb7414cf56957eb72605d42d60926e3ecf85`。当前已验证方案仍使用用户本机原始签名 Games.exe，且其主线程保持挂起，不能把这个文件称作已被开源代码替换。

下一阶段应由原生证据回答：Games.exe 的启动入口、单实例/IPC、配置装载、任务管理和下载 SDK 调用，分别是否为“启动已安装游戏”所必需；完整下载/更新可以作为独立后续目标。以上都是待核实问题，不是本报告已经观测到的运行行为。
