# CMCloud++

CMCloud++ 是基于 Tauri 2、Rust 和 React 的中国移动云盘第三方客户端。以 OpenList 139 驱动为协议参考，提供蓝色重点操作、可收缩侧栏和明暗主题的文件工作台，支持 Windows 和 Android，并保留 iOS 工程兼容性。

当前版本为 `0.1.6`。功能实现范围见 [DRIVER_PARITY.md](DRIVER_PARITY.md)，构建、测试及真实服务联调结果见 [验证记录](docs/VERIFICATION.md)。

源码：[mumingluan/CMCloudPlusPlus](https://github.com/mumingluan/CMCloudPlusPlus) · 下载：[GitHub Releases](https://github.com/mumingluan/CMCloudPlusPlus/releases) · 反馈：[Issues](https://github.com/mumingluan/CMCloudPlusPlus/issues)

## 功能

| 能力 | 范围 |
|---|---|
| 云空间 | 个人云、群组云、分享链接 |
| 文件浏览 | 分页目录、搜索当前已加载内容、排序、缩略图、详情、虚拟列表 |
| 文件管理 | 新建目录、重命名、移动、复制、删除；分享空间为只读 |
| 下载与预览 | 下载链接、文件/文件夹下载、图片与音视频预览；服务端决定链接有效期和预览可用性 |
| 上传 | 文件/文件夹上传、SHA256 秒传协议、分片上传、旧版流式上传 |
| 传输管理 | 持久化队列、暂停、继续、取消、重试、已完成文件打开；Android 可另存导出 |
| 会话管理 | 多账号保存、连接切换、断开与移除凭据；官方 Windows 会话导入、139 邮箱密码、风控短信继续登录、Authorization、邮箱 Cookie |
| 分享 | 提取码、多个分享合并浏览；部分下载需要有效账号 |

群组需要填写对应的 Cloud ID。空间选择显示驱动支持的类型；实际访问取决于账号权限。具体接口和各空间差异见 [驱动移植记录](DRIVER_PARITY.md)。

已有旧版个人云或家庭云配置会自动迁移到个人云，保留登录凭据并重置目录标识。原空间的传输记录继续保留并按空间隔离。

## 开始使用

### Windows：复用官方客户端账号

1. 在本机官方移动云盘客户端登录。
2. 打开 CMCloud++，选择“复用官方客户端账号”，或点击左下角“会话管理”，在登录方式中选择“复用官方 Windows 客户端”；已登录时先展开“添加账号”。
3. 点击“检测并复用凭据”。应用只读解析当前 Windows 用户的官方配置，验证会话后连接默认个人云。
4. 在文件工作台浏览目录；需要群组或分享时，点击左下角“会话管理”，在“切换云空间”选择目标并填写群组 ID 或分享链接，再确认连接。

检测阶段只读取安装路径、版本和配置目录；凭据读取发生在点击导入后。导入不会打开或修改官方 LevelDB 数据库。若提示配置正在更新，关闭官方客户端后重试。官方应用已退出账号或会话失效时，请先在官方应用重新登录。

### 密码与短信继续登录

点击左下角“会话管理”，在登录方式中选择“139 邮箱账号与密码”；已登录时先展开“添加账号”，填写 139 邮箱账号/手机号及密码。首次提交可留空短信验证码；服务返回短信风控要求后，按提示填写收到的验证码，再次提交继续登录。可在可选项中填写已有邮箱设备 Cookie。

短信流程沿用参考驱动的密码登录风控续接。需要图片验证码或官方设备验证时，应在官方渠道完成验证后重试。密码和短信验证码不会持久化保存。

### 其他登录与分享

| 入口 | 使用方法 |
|---|---|
| Authorization | 粘贴有效云盘 Authorization，支持 `Basic` 前缀 |
| 邮箱 Cookie | 填写包含 `Os_SSo_Sid` 和 `RMKEY` 的 139 邮箱 Cookie |
| 分享链接 | 填写分享链接或 link ID；带提取码使用 `link_id#提取码`，多个分享每行一个 |
| 切换云空间 | 左下角“会话管理”进入账号页，复用当前凭据连接目标空间 |

## 会话与外观

左下角“会话管理”列出已保存账号。连接切换成功后才更新当前会话；原账号未完成传输会暂停，回到该账号后可继续。“断开”保留本机凭据；“移除”需要在列表内确认，并删除该条保存的凭据。最多保存 64 个账号，重新登录同一账号更新原条目。旧版单账号加密 vault 自动升级；密码和短信验证码不持久化。

桌面侧栏可收起为图标栏，手机使用图标栏。底部“设置”可选择浅色、深色或跟随系统，偏好会保存；Windows 标题栏同步所选主题。

## 大文件与批量传输

文件大小、偏移和分片编号使用 64 位数值；下载及上传按流读取，不将整个文件载入内存。上传默认选择 100 MiB 分片，大文件选择 512 MiB，也可在设置中指定分片大小。新版上传的 SHA256 校验需要完整读取一次文件，校验耗时取决于文件大小和存储速度；秒传是否成功由服务端决定。

传输页面可选择“单次跳过”或“本会话跳过”，作用于文件和文件夹上传；单次选项在成功加入一批上传后恢复默认，取消选择或下载不会消耗它。设置中关闭“上传前计算文件哈希”可长期保存默认值；策略在加入任务时固定，之后改设置不影响已有任务。当前个人空间接口强制要求完整 SHA256，选择跳过仍会按服务端要求计算，并有提示；群组可通过流式上传路径跳过预计算，失去秒传。本轮真实请求验证了个人空间的限制，没有使用伪造哈希。

上传成功完成后，文件列表自动显示已确认的云端文件；删除按服务端返回的成功项目立即更新，部分失败只重试剩余项目。自动目录同步合并批量通知，保留已加载分页，并隔离旧请求及其他账号、空间和目录的通知。


SQLite WAL 持久化任务及分片进度，任务绑定账号和空间，重启后恢复到匹配的会话。待处理队列最多 100,000 项，每批最多 10,000 项；单次目录扫描最多 100,000 项。传输列表按 200 项分页并虚拟渲染，文件目录逐页加载并虚拟渲染。遇到目录扫描上限时，已入队任务仍可在传输列表查看。同名下载不会覆盖已有本地文件。

实现支持 TB 量级偏移及万级任务，但本轮没有进行实际 TB 文件传输。服务端单文件大小、账号限额、限速和签名地址失效仍会影响传输。

## Android 文件与后台传输

交付 APK 为 arm64，要求 Android 7.0 / API 24 及以上。上传通过系统文档选择器授权：单次选择最多 64 个文件；批量上传建议选择文件夹，一个目录授权即可覆盖其子目录。应用对持久授权设置 128 项预算，避免大量单文件授权耗尽系统资源。

下载先保存到应用私有数据目录的 `Downloads`。完成后可通过 FileProvider 打开，也可在传输列表选择“导出”，使用系统“另存为”选择目标位置。导出以 64 KiB 缓冲流式复制；目标位置需要额外空间，导出期间需保持应用打开，当前导出没有断点续传。卸载应用会清除应用私有下载，需保留的文件请先导出。

传输使用 `dataSync` 前台服务和通知，仍受系统后台时限、电量策略及进程终止影响。Android 15 及以上针对相应目标版本的后台 `dataSync` 服务有累计时限；系统超时后回到应用继续。真机后台长时间传输、SAF 文档提供者兼容性和系统回收恢复仍需实机验证，详见 [验证记录](docs/VERIFICATION.md)。

## 本地开发

需要 Node.js/npm、Rust 1.88 及以上（推荐当前 stable），以及对应平台的 [Tauri 2 前置依赖](https://v2.tauri.app/start/prerequisites/)。Windows 使用 Microsoft C++ Build Tools 的“使用 C++ 的桌面开发”、Windows SDK 和 WebView2；Android 需要 JDK、Android SDK、NDK。当前锁定依赖要求至少 Rust 1.88，本轮使用 Rust 1.92。

获取源码并安装前端依赖：

```powershell
git clone https://github.com/mumingluan/CMCloudPlusPlus.git
cd CMCloudPlusPlus
npm ci
```

Windows 原生开发：

```powershell
. .\scripts\windows-env.ps1
npm run tauri -- dev
```

`windows-env.ps1` 配置本机 Build Tools 2022 MSVC 与 Windows SDK 路径。在另一台电脑使用时，先根据实际安装版本调整该脚本的路径。

界面独立预览：

```powershell
npm run dev
```

浏览器打开 Vite 输出的地址；`?preview=scale` 可查看 10,000 个示例文件的布局。浏览器预览不连接原生登录和传输服务。

## 构建

### Windows

```powershell
.\scripts\build-windows.ps1
```

默认构建 Windows x64 快速 release 与 NSIS 安装包，并显式启用 `custom-protocol` 使用应用内前端资源。可用 `-Debug` 构建调试版本，`-NoBundle` 仅生成可执行文件，`-Production` 使用完整 LTO 发布优化。脚本先加载 `windows-env.ps1`，切换到项目根目录，并在结束或失败后恢复原工作目录。

脚本使用当前 Windows x64 MSVC host，与原生开发复用 `src-tauri/target/release/` 缓存；安装包在其 `bundle/nsis/` 下。交付目录为 `releases/`，Windows 文件命名为 `CMCloudPlusPlus-0.1.5-windows-x64-setup.exe`、`CMCloudPlusPlus-0.1.5-windows-x64.exe`；最终文件及校验值以该目录和 `SHA256SUMS.txt` 为准。

同目录提供 `CMCloudPlusPlus-0.1.5-source.zip`，包含应用、原生插件、图标、锁文件及文档；生成工程、依赖目录和编译缓存由构建命令重新创建。源码打包可复现为 `python .\scripts\package-source.py`。

### 快速迭代

前端交互调整使用 `npm run dev` 或 `npm run tauri -- dev` 热更新；仅查看界面不需要重新生成安装包。需要交付包时，构建脚本默认关闭 LTO、使用 16 个代码生成单元和增量编译，仍生成优化后的 release 二进制。完整发布优化使用 `-Production`。

Windows 统一使用 host 构建目录。Android CLI 先编译并暂存原生库，Gradle 对暂存库与本轮原生输出做 SHA256 比对后复用它；手动 Android Studio 构建继续调用 Cargo。脚本对当前生成工程注入这项检查，未知模板会停止并提示。构建继续串行共享资源锁；不要执行 `cargo clean` 或同时重写 `dist`。切换优化配置会首次重新建立缓存，后续构建才能收益。

本机 0.1.3 Windows 首次快速构建总用时 159.11 秒，随后含前端和窗口主题权限变化的增量安装包构建为 54.36 秒。Android 首个可用增量 APK 用时 97.64 秒，原生编译只占 4.27 秒。修复多账号源路径去重后的最终暖缓存重打包为 Windows 57.92 秒、Android 49.66 秒。数值来自实际执行，不代表其他机器或冷缓存耗时。

### Android

首次创建本机生成工程后构建：

```powershell
. .\scripts\windows-env.ps1
. .\scripts\android-env.ps1
npm run tauri -- android init
.\scripts\build-android.ps1
```

默认构建 arm64 debug APK；`-Release` 选择快速 release，`-Release -Production` 使用完整 LTO 发布优化，`-Target` 可选择其他脚本支持的 ABI。本轮交付的 `CMCloudPlusPlus-0.1.6-android-arm64-preview.apk` 使用预览签名；正式发布需自行配置受控发布密钥。移动端生成工程位于 `src-tauri/gen/`，构建脚本会设置 NDK 链接对齐和本机构建环境。

优化版预览 APK 的复现命令：

```powershell
.\scripts\build-android.ps1 -Release
.\scripts\sign-android-preview.ps1 -Apk .\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release-unsigned.apk -Output .\releases\CMCloudPlusPlus-0.1.6-android-arm64-preview.apk
```

预览签名脚本只使用本机已有的 `.android/debug.keystore`，不创建发布密钥；它会验证签名及 APK 的 16 KiB ZIP 对齐。构建脚本将 JVM 临时目录限定在项目 `.build-tmp`，并关闭 Windows 跨盘 Kotlin 增量缓存，均仅影响当前构建进程。

最终交付预览 APK 的实际大小以交付文件为准；v1/v2/v3 签名、16 KiB 原生 ELF 和 ZIP 对齐均通过检查，并核对了最终前端资源。0.1.5 已在 Android 16 / API 36 设备上覆盖安装并冷启动，桌面图标修复已获用户确认；当前版本的验证范围见 [验证记录](docs/VERIFICATION.md)。

### iOS

在 macOS 安装 Xcode 及 Tauri iOS 前置依赖后，从项目根目录执行 `npm run tauri -- ios init`，再使用 `ios dev` / `ios build`。`tauri.ios.conf.json` 将最低系统版本设为 iOS 14.0。本轮在 Windows 环境完成工程兼容适配，未进行 Xcode 编译、签名或真机运行验证。

iOS 文件夹上传选择器目前尚未适配，调用该入口会返回明确提示；其文档权限与后台传输还需平台适配和真机验证。

## 测试与证据

```powershell
npm test
npm run build
. .\scripts\windows-env.ps1
cargo test --manifest-path .\src-tauri\Cargo.toml --lib --target x86_64-pc-windows-msvc
```

已使用真实账号验证新版个人云会话、路由、目录列表和容量查询；0.1.3 已完成个人云小文本上传、下载回读和删除清理。群组、分享、短信风控及大规模传输仍需对应账号验证。完整测试结果及平台边界见 [docs/VERIFICATION.md](docs/VERIFICATION.md)。

Rust 单元测试 40 项已在 0.1.4 通过，前端测试 19 项已在当前版本通过，其中生产资源回归直接检查 Tauri Context 中的首页及引用 JS/CSS。两个构建脚本共用文件锁，覆盖前端构建与原生打包阶段，避免共享 `dist` 被同时清空和读取；直接调用 Tauri CLI 时应串行构建。

## 凭据与许可证

Windows 凭据通过当前用户的 DPAPI 保护。移动端使用 AES-256-GCM，包装密钥位于应用私有目录并以 Unix `0600` 权限保存；该实现没有硬件 Keystore/Keychain 绑定。接口凭据留在原生层，账号摘要提供给界面；日志不记录密码、Cookie、令牌或带签名下载地址。

项目整体采用 `AGPL-3.0-or-later`，完整许可证见 [LICENSE](LICENSE)，协议参考和来源声明见 [NOTICE.md](NOTICE.md)。参与开发见 [CONTRIBUTING.md](CONTRIBUTING.md)。

0.1.4 含目录同步原生代码与前端更新，实际构建耗时为 Windows 99.15 秒、Android 75.96 秒。

Android 图标在每次构建前从 `src-tauri/icons/android` 同步到生成工程，包含普通、圆形和自适应资源；包验证逐项核对 10 个实际打包 PNG、前景矢量与两个自适应入口。预览签名前执行 16 KiB ZIP 对齐，签名包含 v1/v2/v3，沿用本机开发证书。安装验证可执行：

```powershell
.\scripts\verify-android-device.ps1 -Apk .\releases\CMCloudPlusPlus-0.1.6-android-arm64-preview.apk
```

脚本选择唯一已授权的 ADB 设备与当前用户，以 `--no-streaming -r` 覆盖安装；核对设备上的 APK 字节并启动应用，不卸载或清除数据。有多个设备时使用 `-Serial` 指定。

验证交付包可运行 `python scripts/verify-packages.py`；AAPT 重压缩 PNG 时的逐像素比对需要 Pillow（`python -m pip install Pillow`）。Windows 构建脚本自动发现 Visual Studio C++ 工具链和 Windows SDK。
