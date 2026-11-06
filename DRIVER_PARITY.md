# OpenList 139 驱动移植记录

参考源为 `D:\Library\Projects\OpenList\drivers\139`。本项目使用 AGPL-3.0-or-later；实现参考 OpenList 的协议行为，来源保留在本文。以下覆盖是代码实现范围，真实服务联调需要用户有效登录及对应空间权限。

## 空间与功能

| 功能 | personal_new | group | share |
|---|---|---|---|
| 分页目录、缩略图、元数据 | ✓ 游标 | ✓ 范围 | ✓ 分享目录 |
| 下载地址 | ✓ CDN 优先 | ✓ 路径 | ✓ 登录下载、公开预览 |
| 新建目录、重命名、移动、复制、删除 | ✓ | ✓ | 只读 |
| SHA256 秒传与分片上传 | ✓ | ✓ | — |
| 兼容流式分片上传 | — | ✓ 可选 | — |
| 视频转码预览 | ✓ | 下载地址预览 | ✓ presentURL |
| 配额查询 | userDomainId | userDomainId | 登录后 userDomainId |
| 多分享合并目录、提取码 | — | — | ✓ |

来源：`driver.go:180`（List）、`:203`（Link）、`:237`（MakeDir）、`:297`（Move）、`:411`（Rename）、`:523`（Copy）、`:595`（Remove）、`:688`（Put）、`:1061`（Other）、`:1086`（GetDetails）。本项目对应 `src-tauri/src/cloud.rs`。

默认根目录：个人云 `/`、群组云 `cloudId`、分享 `root`。群组需要填写 `cloudId`。空间选项是驱动类型配置，实际访问取决于账号权限。

## 登录

| 路径 | 输入 | 行为与参考 |
|---|---|---|
| Authorization | Base64 编码的 `pc:账号:令牌` | 单独使用；支持粘贴 Basic 前缀后规范化。`util.go:101`, `:1876` |
| 139 邮箱密码 | 用户名与密码 | `mail.10086.cn/Login/Login.ashx`，密码为 SHA1(`fetion.com.cn:` + password)，Cookie 自动合并。`:1342` |
| 邮箱 Cookie 快速登录 | `Os_SSo_Sid` 与 `RMKEY` | 跳过密码阶段，交换 artifact 后登录云盘。`:1792`, `:1817` |
| 风控短信继续登录 | 上述密码登录触发的验证码 | PML401010062→scene 2、MW0016→scene 4、S025/S035→scene 1；不自动重复发送。`:1185`, `:1234`, `:1280` |
| 官方 PC 凭据导入 | 用户本机官方应用有效凭据 | 额外功能，由本项目平台适配实现；成功导入后仍由同一原生客户端验证 |
| 分享免登录浏览 | linkId / 分享链接与可选提取码 | 下载可能仍要求账号；不伪装登录态。`driver.go:46`, `util.go:780` |

邮箱 artifact 交换：`smsrebuild1.mail.10086.cn/setting/s?func=umc:getArtifact&sid=…`，仅发 RMKEY Cookie。第三方登录：`user-njs.yun.139.com/user/thirdlogin`，响应双层解密后生成 Authorization，并保存 userDomainId。来源 `util.go:1436`, `:1715`。

没有在此参考驱动中发现原生扫码登录或手机号直接短信登录接口；风控图片验证码需要官方验证。本项目不把这些未验证接口标为已实现。

## 请求协议与加密

一般 API POST JSON，Authorization 为 `Basic <base64>`，完整 mcloud 与 X-Yun 客户端头沿用参考驱动。签名算法：JS encodeURIComponent(body) → 排序字符 → Base64 → MD5；拼接 MD5(timestamp + ':' + nonce)，再 MD5 后大写。时间固定中国时区，nonce 16 位。`util.go:74–94`, `:160`, `:846`。

加密 API：排序 JSON、PKCS7、AES-CBC，随机 16 字节 IV，发送 Base64(IV + 密文)；支持加密响应与普通 JSON 错误响应。登录第一层与 andAlbum 使用 AES-192，登录内层 AES-128-ECB，分享 API 使用独立 AES-128 密钥。短信用户名 RSA PKCS#1 v1.5；密码/验证码 SHA1。仅用于现有服务兼容，凭据不写日志。`util.go:1198`, `:1526–1713`。

令牌内 `|` 分隔字段第 4 项为 Unix 毫秒有效期；剩余超过 15 天不续期，临近有效期经 XML `aas.caiyun.feixin.10086.cn:443/tellin/authTokenRefresh.do`，clienttype 656 续期。失效时密码回退。OpenList 使用 12 小时后台周期；本客户端由应用后台周期调用原生续期，并持久化结果。`util.go:101`, `driver.go:100`。

路由查询：`user-njs.yun.139.com/user/route/qryRoutePolicy` 发现个人与群组所需的 HTTPS 主机；其中服务端 `family` 路由用于群组的共用相册复制协议。本客户端验证 HTTPS 及 yun.139.com 域名边界；群组上传使用 group 主机。`driver.go:63`, `util.go:233`, `:913`。

## 端点矩阵

新版个人路由主机：`/file/list`、`/file/getDownloadUrl`、`/file/create`、`/file/update`、`/file/batchMove`、`/file/batchCopy`、`/recyclebin/batchTrash`、`/videoPreview/getPreviewInfo`。目录 `pageInfo.pageCursor` / `nextPageCursor`，每页 100。来源 `util.go:942`, `:1009`。

共享群（yun.139.com）：`/orchestration/group-rebuild/content/v1.0/queryGroupContentList`（1–100、101–200…）、`groupManage/v1.0/getGroupFileDownLoadURL`、`catalog/v1.0/createGroupCatalog`、`catalog/v1.0/modifyGroupCatalog`、`content/v1.0/modifyGroupContent`、`task/v1.0/createBatchOprTask`。复制使用共用相册路由 `/andAlbum/openApi/copyContentCatalog`，来源 `util.go:423`, `:508`, `:1957`。

分享（share-kd-njs.yun.139.com/yun-share）：`/richlifeApp/devapp/IOutLink/getOutLinkInfoV6`、`getContentInfoFromOutLink`、`dlFromOutLinkV3`。逗号、分号、换行分隔多个分享，`linkId#提取码`；同名目录合并，同名文件保留首项。来源 `util.go:524–838`。

配额：`user-njs.yun.139.com/user/disk/quota/detail`，容量单位 MiB，需求 userDomainId。来源 `util.go:1127`。

## TB 大文件与万级目录

上传全程使用 u64 字节/分片偏移。默认 100 MiB，超过驱动的 30 GiB 级别阈值改为 512 MiB；可自定义。分片 URL 每批最多 100 个，不把大文件读入内存。SHA256 流式计算，任务与已完成分片持久化，进度按确认字节计数。大型文件可暂停、恢复、取消；过期分片 URL 重新取得。秒传和已存在响应跳过传输。

新版上传：个人 `/file/{create,getUploadUrl,complete}`；群组 group 路由 `/dynamic/file/{create,getUploadUrl,complete}`，加 groupId/groupType 2/catalogType 3/seqNo。URL PUT octet-stream，明确 Content-Length。来源 `driver.go:677–890`, `util.go:1063`。

群组兼容上传仍沿用驱动共用的 `/orchestration/familyCloud-rebuild/content/v1.0/getFileUploadURL` 申请接口；该端点用于群组上传。申请 redirectionUrl/uploadTaskID，POST 分片，Content-Type 包含 ASCII/Unicode 转义文件名，contentSize/range/uploadtaskID/rangeType 头，检查 XML resultCode。无完整文件秒传。来源 `driver.go:893–1055`。

目录分页在原生层独立暴露，UI 可逐页渲染并虚拟化万级条目。list_all 只在批量操作需要时使用，并检测重复游标和没有新条目的重复页。

下载注意服务端可能返回 HTTP 200 但 Content-Range 正确，恢复逻辑应先验证 Content-Range 起点；不盲目把 200 响应追加到已有文件。OpenList 对应 `internal/stream/util.go:136`。

## 移植时修正的风险

- 群组复制使用当前账号 userDomainId，避免参考实现 `d.ref.UserDomainID` 的空引用。
- 响应时间作为字符串保留，避免参考实现时间解析 panic。
- 校验解密响应业务错误与 PKCS7 全部填充字节；限制响应格式与分片编号。
- 分页检测重复游标；对空页/末页终止。分享全部失败时明确报错。
- 上传保留服务端冲突自动改名，避免参考驱动先重命名/删除旧文件的自动覆盖流程；用户可明确选择删除或重命名。
- 令牌、Cookie、短信、密码、带签名下载 URL 不写入诊断日志。

尚需真实账号验证的边界：运营商近期协议变更、群组账号权限、风控验证码、服务端秒传及签名 URL 失效、超大文件的服务端大小限制。此记录不等同于线上通过报告。

## 当前验证记录

2026-10-05 使用本机官方客户端当前会话进行只读验证：新版个人登录/路由初始化、根目录与五个子目录列表、容量查询通过。上述目录未包含可用于测试的普通文件，因此没有验证下载 URL 或传输。未进行远端新建、上传、重命名、移动、复制、删除。

签名与 AES-192-CBC 使用独立 Go 标准库生成的固定样例对照；原生认证/协议及官方凭据解析模块的 16 个单元测试在 Windows GNU 工具链通过。完整应用构建与其他模块测试由项目验证记录另行说明。


## 客户端会话和上传校验

0.1.3 在本机加密 vault 中保存多个账号，原单账号格式自动迁移，切换成功后才提交当前会话，断开保留账号，移除删除凭据。任务仍按账号、空间、群组/分享标识隔离。

所有上传始终预计算完整 SHA256。恢复上传时重新计算校验值，并保留源文件变化检查。个人空间接口要求完整 SHA256；此前个人空间小文件创建、分片、完成、下载回读和清理验证通过。
