import copy
import json
import tempfile
import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import cmcloud_mirror as mirror


def item(identifier, name, size, parent="root", directory=False):
    return {"id": identifier, "name": name, "size": size, "path": parent,
        "isDir": directory, "modifiedAt": ""}


class FakeCloud:
    def __init__(self, fail_upload=False):
        self.entries = {"old": item("old", "changed.bin", 3), "extra": item("extra", "extra.bin", 4)}
        self.lock, self.events, self.fail_upload = threading.Lock(), [], fail_upload
        self.counter = 0

    def listing(self, parent):
        with self.lock:
            return copy.deepcopy([entry for entry in self.entries.values() if entry["path"] == parent["id"]])

    def call(self, op, **data):
        with self.lock:
            if op == "upload":
                if self.fail_upload:
                    raise RuntimeError("simulated network failure")
                self.counter += 1
                identifier = "new-" + str(self.counter)
                self.entries[identifier] = item(identifier, data["name"], data["size"], data["parent"]["id"])
                self.events.append(("upload", identifier))
                return {"id": identifier, "size": data["size"], "rapid": False}
            if op == "remove":
                identifier = data["file"]["id"]
                self.events.append(("remove", identifier))
                self.entries.pop(identifier)
                return True
            if op == "rename":
                identifier = data["file"]["id"]
                self.entries[identifier]["name"] = data["name"]
                self.events.append(("rename", identifier))
                return True
            raise AssertionError(op)


class MirrorTests(unittest.TestCase):
    def test_same_size_is_equal_without_comparing_modified_time_or_hash(self):
        local = {"a": {"kind": "file", "size": 10, "mtime": 123}}
        plan = mirror.make_plan(local, {"a": item("a", "a", 10)})
        self.assertEqual(plan["equal"], ["a"])
        self.assertFalse(plan["replace"])

    def test_extra_subtree_is_deleted_once_and_type_conflict_children_are_not_separate_deletions(self):
        local = {"conflict": {"kind": "file", "size": 10, "mtime": 1}}
        remote = {"conflict": item("c", "conflict", 0, directory=True),
            "conflict/child": item("cc", "child", 1, "c"),
            "extra": item("e", "extra", 0, directory=True), "extra/child": item("ec", "child", 2, "e")}
        plan = mirror.make_plan(local, remote)
        self.assertEqual(plan["replace"], ["conflict"])
        self.assertEqual(plan["delete"], ["extra"])

    def run_execute(self, fail=False):
        with tempfile.TemporaryDirectory(prefix="cmcloud-mirror-test-", dir="R:/Temp") as temporary:
            root = Path(temporary) / "local"
            work = Path(temporary) / "work"
            root.mkdir(); work.mkdir()
            (root / "changed.bin").write_bytes(b"new-data")
            (root / "new.bin").write_bytes(b"hello")
            cloud = FakeCloud(fail)
            target = item("root", "Diezhi", 0, parent="/", directory=True)
            ready = {"accountId": "fixture", "space": "personal_new", "root": target}
            class FakeBridge:
                def __init__(self, *args, **kwargs): self.ready, self.current = ready, ""
                listing = lambda self, parent: cloud.listing(parent)
                call = lambda self, op, **data: cloud.call(op, **data)
                close = lambda self: None
            bridge = FakeBridge()
            local = mirror.local_inventory(root)
            remote = {entry["name"]: entry for entry in cloud.listing(target)}
            plan = mirror.make_plan(local, remote)
            log = mirror.Log(work)
            try:
                with patch.object(mirror, "Bridge", FakeBridge), patch.object(mirror, "active_writers", return_value=0):
                    if fail:
                        with self.assertRaisesRegex(RuntimeError, "未执行多余项目删除"):
                            mirror.execute(SimpleNamespace(workers=1), root, work, log, bridge, target, local, remote, {"": target}, plan)
                        self.assertIn("old", cloud.entries)
                        self.assertIn("extra", cloud.entries)
                    else:
                        mirror.execute(SimpleNamespace(workers=1), root, work, log, bridge, target, local, remote, {"": target}, plan)
                        final = {entry["name"]: entry["size"] for entry in cloud.entries.values()}
                        self.assertEqual(final, {"changed.bin": 8, "new.bin": 5})
                        self.assertLess(next(i for i, event in enumerate(cloud.events) if event[0] == "upload"), cloud.events.index(("remove", "old")))
                        self.assertGreater(cloud.events.index(("remove", "extra")), max(i for i, event in enumerate(cloud.events) if event[0] == "rename"))
                        self.assertTrue(json.loads((work / "result.json").read_text())["verified"])
            finally:
                log.stop.set()

    def test_stages_replacements_before_removing_old_files_and_deletes_extras_last(self):
        self.run_execute()

    def test_failed_upload_keeps_the_old_file_and_all_extras(self):
        self.run_execute(True)


if __name__ == "__main__":
    unittest.main()
