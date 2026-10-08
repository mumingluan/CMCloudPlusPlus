# 来源与许可证声明

CMCloud++ 的中国移动云盘协议实现参考 OpenList 的 139 驱动，并将相关请求、认证、签名、加密、空间及上传行为移植为 Rust 实现。项目整体采用 `AGPL-3.0-or-later`，完整许可证位于 [LICENSE](LICENSE)。

## OpenList 参考来源

- 上游项目：[OpenListTeam/OpenList](https://github.com/OpenListTeam/OpenList)。
- 参考提交：[`21aa31904e2dfd6e0b3217ec843c5f162bc9694d`](https://github.com/OpenListTeam/OpenList/tree/21aa31904e2dfd6e0b3217ec843c5f162bc9694d)。
- 主要参考：[drivers/139/driver.go](https://github.com/OpenListTeam/OpenList/blob/21aa31904e2dfd6e0b3217ec843c5f162bc9694d/drivers/139/driver.go)、[drivers/139/util.go](https://github.com/OpenListTeam/OpenList/blob/21aa31904e2dfd6e0b3217ec843c5f162bc9694d/drivers/139/util.go)，以及下载 Range 行为相关的 `internal/stream/util.go`。
- 功能与接口映射：[DRIVER_PARITY.md](DRIVER_PARITY.md)。

保留 OpenList 上游及其贡献者的权利和许可证声明。本项目的 [LICENSE](LICENSE) 是参考仓库 GNU Affero General Public License version 3 全文的原样副本；没有修改许可证正文。2026-10-05 核对两份文件的 SHA256 相同：`6f1e622c82a380075843bb084a7ec3b1f1d12a4a02526d75e78b0924a860aa75`。

Rust 协议移植与本项目其余代码一并按项目声明的 AGPL 许可提供。分发、修改及通过网络提供修改版本时，应遵守完整许可证对源代码和许可声明的要求。

## 项目身份与资源

CMCloud++ 是第三方客户端，名称及文档用于说明其所连接的中国移动云盘服务。OpenList 和中国移动的名称、服务与标识归各自权利人所有。

项目依赖的 Rust crates、npm 包及 Android/Tauri 组件遵循各自许可证。原生依赖和前端依赖的版本分别记录于 `src-tauri/Cargo.lock`、`package-lock.json`。应用图标由本项目原创 [source.svg](src-tauri/icons/source.svg) 经官方 Tauri CLI 转换，PNG 内嵌来源元数据；ICO 和 ICNS 的来源另记录于各自的 JSON 旁车文件。
