# Diezhi 大小镜像同步

在当前 Windows 用户下读取 CMCloud++ 已保存的登录凭据，将 `D:/Diezhi` 镜像到当前账号根目录中已有的 `Diezhi` 文件夹。凭据只在桥接进程内存中解密，不写入脚本、命令行或日志。

```powershell
# 清点和生成计划，不修改云端
./scripts/sync-diezhi.ps1

# 执行镜像，并在最后复核
./scripts/sync-diezhi.ps1 -Apply

# 桥接程序已构建时跳过构建
./scripts/sync-diezhi.ps1 -Apply -SkipBuild -Workers 4
```

同相对路径、同类型且同大小的文件跳过，不比较修改时间或内容哈希。新增文件和大小不同的文件才读取内容、计算上传协议所需的 SHA256。文件夹层级和空文件夹会保留。

新增及覆盖文件先上传到目标父目录下的暂存名称。完成上传并确认大小后，才删除旧的同路径项目并改回本地名称。所有上传成功后，再删除本地不存在的远端项目；API 使用云盘的回收站操作，不清空回收站。最后重新清点两边，检查路径、类型和文件大小完全一致。

清点失败、重名路径、链接/重解析点、源文件变化、上传失败或 CMCloud++ 同时向目标目录上传时会停止。上传失败不会进入多余项目删除阶段。同一工作目录只能运行一个脚本。

日志及断点默认位于 `R:/Temp/cmcloud-mirror-diezhi`：

- `plan.json`：计划与字节数。
- `local-manifest.json`、`remote-manifest.json`：清点元数据。
- `events.jsonl`、`status.json`：事件和运行状态。
- `upload-*.json`：分片断点，保留后可重新执行脚本继续。
- `result.json`：最终复核成功后生成，包含 `verified: true`。
- `failures.json` 或 `remaining-differences.json`：未完成时的原因。

状态中的文件完成字节数表示已同步文件的逻辑大小；分片进度位于 `current`，秒传不等于同等网络流量。工作目录需要在续跑时保留；如果临时盘被清空，会重新清点并重新建立上传会话。

桥接程序源码为 `src-tauri/examples/cmcloud_sync_bridge.rs`，复用项目中的认证及云盘协议模块；本工具不修改应用凭据库、应用传输队列或本地源文件。
