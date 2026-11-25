"""Mirror D:/Diezhi to its CMCloud++ folder, comparing existing files by size.

The default is a read-only plan. --apply executes that plan and verifies the tree.
Credentials are decrypted only by the local Rust bridge, never written here.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import sqlite3
import stat
import subprocess
import threading
import time

PROJECT = Path(__file__).resolve().parents[1]
BRIDGE = PROJECT / "src-tauri/target/release/examples/cmcloud_sync_bridge.exe"


def atomic_json(path, value):
    temporary = Path(str(path) + ".tmp")
    with temporary.open("w", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def clean_file(value):
    return {key: value[key] for key in ("id", "name", "path", "isDir", "size", "modifiedAt")}


class Log:
    def __init__(self, work):
        self.work = work
        self.lock = threading.RLock()
        self.state = {"phase": "planning", "filesDone": 0, "bytesDone": 0, "current": {}}
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.heartbeat, daemon=True)
        self.thread.start()

    def event(self, kind, **data):
        record = {"time": time.strftime("%Y-%m-%d %H:%M:%S"), "event": kind, **data}
        with self.lock:
            with (self.work / "events.jsonl").open("a", encoding="utf-8") as stream:
                stream.write(json.dumps(record, ensure_ascii=False) + "\n")
            print(json.dumps(record, ensure_ascii=False), flush=True)

    def progress(self, name, value):
        with self.lock:
            self.state["current"][threading.current_thread().name] = {"path": name, **value}

    def heartbeat(self):
        while not self.stop.wait(20):
            with self.lock:
                snapshot = json.loads(json.dumps(self.state))
                atomic_json(self.work / "status.json", snapshot)
            self.event("status", **snapshot)


class Bridge:
    def __init__(self, local, work, log, expected=None):
        self.log, self.current = log, ""
        self.stderr = (work / ("bridge-" + threading.current_thread().name + ".log")).open("a", encoding="utf-8")
        self.process = subprocess.Popen([str(BRIDGE), str(local), str(work)], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=self.stderr, text=True, encoding="utf-8", bufsize=1)
        self.ready = self.read()
        if self.ready.get("kind") != "ready":
            self.close()
            raise RuntimeError(self.ready.get("error", "同步接口无法启动"))
        if expected and any(self.ready[key] != expected[key] for key in ("accountId", "space", "root")):
            self.close()
            raise RuntimeError("CMCloud++ 当前账号或空间发生变化，停止同步")

    def read(self):
        line = self.process.stdout.readline()
        if not line:
            raise RuntimeError("同步接口意外退出；请检查 bridge 日志")
        return json.loads(line)

    def call(self, op, **kwargs):
        self.process.stdin.write(json.dumps({"op": op, **kwargs}, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        while True:
            result = self.read()
            if result.get("kind") == "progress":
                self.log.progress(self.current, result)
                continue
            if not result.get("ok"):
                raise RuntimeError(result.get("error", "云端操作失败"))
            return result["value"]

    def listing(self, parent):
        for attempt in range(4):
            try:
                return [clean_file(item) for item in self.call("list", parent=parent)]
            except RuntimeError:
                if attempt == 3:
                    raise
                time.sleep(2 ** attempt)

    def close(self):
        try:
            self.process.stdin.close()
            self.process.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired):
            self.process.terminate()
            self.process.wait(timeout=10)
        self.stderr.close()


def local_inventory(root):
    result = {}
    pending = [root]
    while pending:
        directory = pending.pop()
        for entry in os.scandir(directory):
            metadata = entry.stat(follow_symlinks=False)
            if metadata.st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                raise RuntimeError("发现链接或重解析点，清点未完成：" + entry.path)
            relative = Path(entry.path).relative_to(root).as_posix()
            if stat.S_ISDIR(metadata.st_mode):
                result[relative] = {"kind": "dir", "size": 0}
                pending.append(Path(entry.path))
            elif stat.S_ISREG(metadata.st_mode):
                result[relative] = {"kind": "file", "size": metadata.st_size, "mtime": metadata.st_mtime_ns}
            else:
                raise RuntimeError("不支持的本地项目，清点未完成：" + entry.path)
    if not result:
        raise RuntimeError("本地目录为空，拒绝自动清空远端")
    return result


def remote_inventory(bridge, target, log):
    result, directories = {}, {"": target}
    pending, visited = [("", target)], set()
    for prefix, directory in pending:
        if directory["id"] in visited:
            raise RuntimeError("远端目录出现循环或重复标识")
        visited.add(directory["id"])
        children = bridge.listing(directory)
        for item in children:
            name = item["name"]
            if not name or name in (".", "..") or "/" in name or "\\" in name:
                raise RuntimeError("远端名称无法安全映射为相对路径：" + name)
            path = prefix + "/" + name if prefix else name
            if path in result:
                raise RuntimeError("远端同一路径存在重名项目：" + path)
            result[path] = item
            if item["isDir"]:
                directories[path] = item
                pending.append((path, item))
        log.event("inventory", directory=prefix or ".", items=len(result))
    return result, directories


def underneath(path, parent):
    return path == parent or path.startswith(parent + "/")


def make_plan(local, remote):
    plan = {"equal": [], "upload": [], "replace": [], "mkdir": [], "delete": []}
    replaced_directories = set()
    for path, source in local.items():
        destination = remote.get(path)
        if source["kind"] == "dir":
            if not destination or not destination["isDir"]:
                plan["mkdir"].append(path)
        elif destination and not destination["isDir"] and destination["size"] == source["size"]:
            plan["equal"].append(path)
        else:
            plan["replace" if destination else "upload"].append(path)
            if destination and destination["isDir"]:
                replaced_directories.add(path)
    extras = [path for path in remote if path not in local and not any(underneath(path, parent) for parent in replaced_directories)]
    for path in sorted(extras, key=lambda value: (value.count("/"), value)):
        if not any(underneath(path, parent) for parent in plan["delete"]):
            plan["delete"].append(path)
    return plan


def summary(local, remote, plan):
    return {"localFiles": sum(item["kind"] == "file" for item in local.values()),
        "localDirectories": sum(item["kind"] == "dir" for item in local.values()),
        "localBytes": sum(item["size"] for item in local.values()),
        "remoteFiles": sum(not item["isDir"] for item in remote.values()),
        "remoteDirectories": sum(item["isDir"] for item in remote.values()),
        "remoteBytes": sum(item["size"] for item in remote.values() if not item["isDir"]),
        "sameSizeFiles": len(plan["equal"]), "newFiles": len(plan["upload"]),
        "replaceFiles": len(plan["replace"]), "createDirectories": len(plan["mkdir"]),
        "deleteOperations": len(plan["delete"]),
        "uploadBytes": sum(local[path]["size"] for path in plan["upload"] + plan["replace"])}


def active_writers(target, directories):
    database = Path(os.environ["APPDATA"]) / "io.cmcloud.plus/transfers.sqlite3"
    ids = {item["id"] for item in directories.values()} | {target["id"]}
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as connection:
        rows = connection.execute("SELECT remote_json FROM transfers WHERE direction='upload' AND status IN ('running','queued')").fetchall()
    return sum(json.loads(row[0]).get("id") in ids for row in rows)


def find_entry(bridge, parent, *, identifier=None, name=None, required=False):
    matches = [entry for entry in bridge.listing(parent)
        if (identifier is None or entry["id"] == identifier) and (name is None or entry["name"] == name)]
    if len(matches) > 1:
        raise RuntimeError("远端项目重名，停止同步：" + str(name))
    if required and not matches:
        raise RuntimeError("远端项目未出现：" + str(name or identifier))
    return matches[0] if matches else None


def wait_entry(bridge, parent, identifier, name):
    for attempt in range(10):
        entry = find_entry(bridge, parent, identifier=identifier, name=name)
        if entry:
            return entry
        time.sleep(min(attempt + 1, 4))
    raise RuntimeError("操作后远端项目未出现：" + name)


def remove_remote(bridge, parent, item, relative, root, target, log, *, replacement=False):
    if item["id"] == target["id"]:
        raise RuntimeError("禁止删除同步根目录")
    candidate = root.joinpath(*PurePosixPath(relative).parts)
    if not candidate.resolve().is_relative_to(root):
        raise RuntimeError("本地路径解析越过同步根目录：" + relative)
    if not replacement and os.path.lexists(candidate):
        raise RuntimeError("本地项目已出现，停止远端删除：" + relative)
    current = find_entry(bridge, parent, identifier=item["id"])
    if not current:
        return False
    if current["name"] != item["name"] or current["isDir"] != item["isDir"]:
        raise RuntimeError("远端项目在清点后发生变化：" + relative)
    for attempt in range(4):
        try:
            bridge.call("remove", file=current)
        except RuntimeError:
            if not find_entry(bridge, parent, identifier=current["id"]):
                break
            if attempt == 3:
                raise
            time.sleep(2 ** attempt)
            continue
        break
    for attempt in range(10):
        if not find_entry(bridge, parent, identifier=current["id"]):
            log.event("remove", path=relative, directory=item["isDir"], replacement=replacement)
            return True
        time.sleep(min(attempt + 1, 4))
    raise RuntimeError("删除后项目仍在原目录：" + relative)


def parent_key(relative):
    parent = PurePosixPath(relative).parent.as_posix()
    return "" if parent == "." else parent


def signatures(entries):
    return {path: (item["kind"], item["size"]) for path, item in entries.items()}


def execute(args, root, work, log, bridge, target, local, remote, directories, plan):
    if active_writers(target, directories):
        raise RuntimeError("CMCloud++ 仍有向该目录写入的上传任务，请先暂停这些任务后重试")
    if signatures(local_inventory(root)) != signatures(local):
        raise RuntimeError("本地目录在清点后发生变化，未开始同步")
    log.state["phase"] = "creating_directories"
    for relative in sorted(plan["mkdir"], key=lambda value: (value.count("/"), value)):
        parent = directories[parent_key(relative)]
        name = PurePosixPath(relative).name
        existing = find_entry(bridge, parent, name=name)
        if existing and not existing["isDir"]:
            original = remote.get(relative)
            if not original or original["id"] != existing["id"]:
                raise RuntimeError("远端结构在清点后发生变化：" + relative)
            remove_remote(bridge, parent, existing, relative, root, target, log, replacement=True)
            existing = None
        if not existing:
            created = bridge.call("mkdir", parent=parent, name=name)
            existing = wait_entry(bridge, parent, created["id"], name)
            log.event("mkdir", path=relative)
        directories[relative] = existing

    clients, client_lock, thread_state = [], threading.Lock(), threading.local()
    stop = threading.Event()
    retained = {item["id"] for path, item in remote.items() if path in plan["equal"]}

    def worker_bridge():
        if not hasattr(thread_state, "client"):
            thread_state.client = Bridge(root, work, log, expected=bridge.ready)
            with client_lock:
                clients.append(thread_state.client)
        return thread_state.client

    def synchronize_file(relative):
        if stop.is_set():
            return None
        current_bridge = worker_bridge()
        current_bridge.current = relative
        source = local[relative]
        parent = directories[parent_key(relative)]
        name = PurePosixPath(relative).name
        key = hashlib.sha256((target["id"] + "\0" + parent["id"] + "\0" + relative + "\0" + str(source["size"]) + "\0" + str(source["mtime"])).encode()).hexdigest()
        stage_name = ".cmcloud-sync-" + key[:24] + ".part"
        journal = work / ("upload-" + key + ".json")
        checkpoint = json.loads(journal.read_text(encoding="utf-8")) if journal.exists() else None
        existing = find_entry(current_bridge, parent, name=name)
        if existing and not existing["isDir"] and existing["size"] == source["size"]:
            log.event("already_equal", path=relative)
            return existing["id"]
        if checkpoint and checkpoint.get("complete"):
            uploaded = find_entry(current_bridge, parent, identifier=checkpoint["plan"]["fileId"])
            if not uploaded or uploaded["size"] != source["size"]:
                journal.unlink()
                checkpoint = None
        if not checkpoint:
            stale_stage = find_entry(current_bridge, parent, name=stage_name)
            if stale_stage:
                stage_relative = (parent_key(relative) + "/" if parent_key(relative) else "") + stage_name
                remove_remote(current_bridge, parent, stale_stage, stage_relative, root, target, log)
        log.event("upload_start", path=relative, bytes=source["size"], replacement=relative in plan["replace"])
        result = current_bridge.call("upload", parent=parent, path=str(root.joinpath(*PurePosixPath(relative).parts)),
            name=stage_name, size=source["size"], mtime=source["mtime"], key=key)
        staged = wait_entry(current_bridge, parent, result["id"], stage_name)
        if staged["isDir"] or staged["size"] != source["size"]:
            raise RuntimeError("上传完成后的文件大小不匹配，旧文件保留：" + relative)
        existing = find_entry(current_bridge, parent, name=name)
        if existing:
            original = remote.get(relative)
            if not original or existing["id"] != original["id"]:
                raise RuntimeError("覆盖前发现远端项目已变化，已保留上传暂存文件：" + relative)
            remove_remote(current_bridge, parent, existing, relative, root, target, log, replacement=True)
        current_bridge.call("rename", file=staged, name=name)
        completed = wait_entry(current_bridge, parent, result["id"], name)
        if completed["isDir"] or completed["size"] != source["size"]:
            raise RuntimeError("改名后文件大小不匹配：" + relative)
        with log.lock:
            log.state["filesDone"] += 1
            log.state["bytesDone"] += source["size"]
            log.state["current"].pop(threading.current_thread().name, None)
        log.event("upload_complete", path=relative, bytes=source["size"], rapid=result["rapid"])
        return completed["id"]

    log.state["phase"] = "uploading"
    jobs = sorted(plan["upload"] + plan["replace"], key=lambda path: local[path]["size"], reverse=True)
    failures = []
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, min(args.workers, 8)), thread_name_prefix="mirror") as pool:
            futures = {pool.submit(synchronize_file, path): path for path in jobs}
            for future in concurrent.futures.as_completed(futures):
                try:
                    identifier = future.result()
                    if identifier:
                        retained.add(identifier)
                except Exception as error:
                    stop.set()
                    failures.append({"path": futures[future], "error": str(error)})
                    log.event("upload_error", **failures[-1])
        if failures:
            atomic_json(work / "failures.json", failures)
            raise RuntimeError("文件同步失败；未执行多余项目删除，请检查 failures.json")
    finally:
        for client in clients:
            client.close()
    if signatures(local_inventory(root)) != signatures(local):
        raise RuntimeError("本地目录在上传期间发生变化，未执行多余项目删除")
    if active_writers(target, directories):
        raise RuntimeError("检测到 CMCloud++ 同时向目标上传，未执行多余项目删除")
    log.state["phase"] = "deleting_extras"
    for relative in plan["delete"]:
        item = remote[relative]
        if item["id"] in retained:
            continue
        remove_remote(bridge, directories[parent_key(relative)], item, relative, root, target, log)
    log.state["phase"] = "verifying"
    for attempt in range(4):
        final, _ = remote_inventory(bridge, target, log)
        differences = make_plan(local_inventory(root), final)
        if not any(differences[key] for key in ("upload", "replace", "mkdir", "delete")):
            log.state["phase"] = "completed"
            report = {"verified": True, "files": sum(item["kind"] == "file" for item in local.values()),
                "directories": sum(item["kind"] == "dir" for item in local.values()),
                "bytes": sum(item["size"] for item in local.values()),
                "uploadedFiles": log.state["filesDone"], "uploadedBytes": log.state["bytesDone"]}
            atomic_json(work / "result.json", report)
            log.event("completed", **report)
            return
        atomic_json(work / "remaining-differences.json", differences)
        time.sleep(2 ** attempt)
    raise RuntimeError("同步后仍存在差异，见 remaining-differences.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local", default="D:/Diezhi")
    parser.add_argument("--remote", default="Diezhi")
    parser.add_argument("--work-dir", default="R:/Temp/cmcloud-mirror-diezhi")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    local_root, work = Path(args.local).resolve(strict=True), Path(args.work_dir).resolve()
    if not local_root.is_dir() or local_root.parent == local_root:
        raise RuntimeError("本地根目录无效")
    if work.is_relative_to(local_root):
        raise RuntimeError("日志和断点目录必须放在本地同步目录之外")
    work.mkdir(parents=True, exist_ok=True)
    import msvcrt
    process_lock = (work / "mirror.lock").open("a+b")
    process_lock.seek(0)
    if not process_lock.read(1):
        process_lock.write(b"0")
        process_lock.flush()
    process_lock.seek(0)
    try:
        msvcrt.locking(process_lock.fileno(), msvcrt.LK_NBLCK, 1)
    except OSError as error:
        process_lock.close()
        raise RuntimeError("该同步工作目录已有脚本在执行") from error
    log, bridge = Log(work), None
    try:
        local = local_inventory(local_root)
        atomic_json(work / "local-manifest.json", local)
        bridge = Bridge(local_root, work, log)
        roots = [item for item in bridge.listing(bridge.ready["root"]) if item["name"] == args.remote and item["isDir"]]
        if len(roots) != 1:
            raise RuntimeError("当前账号根目录中目标文件夹不存在或重名，未修改远端")
        target = roots[0]
        remote, directories = remote_inventory(bridge, target, log)
        plan = make_plan(local, remote)
        report = {"target": target, "accountId": bridge.ready["accountId"], "space": bridge.ready["space"],
            "summary": summary(local, remote, plan), "actions": plan}
        atomic_json(work / "remote-manifest.json", remote)
        atomic_json(work / "plan.json", report)
        log.event("plan", **report["summary"])
        if args.apply:
            execute(args, local_root, work, log, bridge, target, local, remote, directories, plan)
        else:
            log.event("planned", path=str(work / "plan.json"))
    except BaseException as error:
        if 'log' in locals():
            log.state["phase"] = "failed"
            log.event("failed", error=str(error))
        raise
    finally:
        log.stop.set()
        if bridge:
            bridge.close()
        atomic_json(work / "status.json", log.state)
        process_lock.close()


if __name__ == "__main__":
    main()
