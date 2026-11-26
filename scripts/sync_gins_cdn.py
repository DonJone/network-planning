#!/usr/bin/env python3
"""
Gins-Rules CDN 快照同步
拉取 config.py 中 gins 字段引用的 CDN 规则 (rules.ichimarugin728.dev)，
归一化排序后写入 gins-rules/cdn/shadowrocket/，仅在内容变化时落盘，并输出新增/删除统计。
拉取失败时保留原快照，不会清空文件。
"""

import os
import sys
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from config import SOURCES, GINS_CDN_BASE

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GINS_CDN_DIR = os.path.join(BASE_DIR, "gins-rules", "cdn", "shadowrocket")


def cdn_path(ref: str) -> str:
    category, name = ref.split("/", 1)
    if category == "asn":
        category = "ip"
    return f"{category}/{name}"


def normalize(text: str) -> list:
    lines = {line.strip() for line in text.splitlines()}
    return sorted(line for line in lines if line and not line.startswith(("#", "//", ";")))


def fetch(url: str, retries: int = 3, timeout: int = 30):
    headers = {"User-Agent": "network-planning-gins-cdn-sync"}
    for attempt in range(1, retries + 1):
        try:
            req = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return resp.read().decode("utf-8", errors="ignore")
        except Exception as e:
            if attempt == retries:
                print(f"  [WARN] 拉取失败: {url} -> {e}")
                return None
            time.sleep(2)
    return None


def sync(ref: str):
    path = cdn_path(ref)
    text = fetch(f"{GINS_CDN_BASE}/{path}.list")
    new = normalize(text) if text is not None else []
    if not new:
        return ref, None

    snapshot = os.path.join(GINS_CDN_DIR, f"{path}.list")
    old = []
    if os.path.exists(snapshot):
        with open(snapshot, "r", encoding="utf-8") as f:
            old = normalize(f.read())

    added = len(set(new) - set(old))
    removed = len(set(old) - set(new))
    if new != old:
        os.makedirs(os.path.dirname(snapshot), exist_ok=True)
        with open(snapshot, "w", encoding="utf-8") as f:
            f.write("\n".join(new) + "\n")
    return ref, (len(new), added, removed)


def main():
    refs = list(dict.fromkeys(ref for cat in SOURCES.values() for ref in cat.get("gins", [])))
    print(f"[*] 同步 Gins-Rules CDN 快照 ({len(refs)} 个规则集)...")

    with ThreadPoolExecutor(max_workers=8) as executor:
        results = list(executor.map(sync, refs))

    rows = []
    changed = False
    failed = []
    for ref, stat in results:
        if stat is None:
            failed.append(ref)
            rows.append(f"| {ref} | - | - | - | 拉取失败，保留原快照 |")
            print(f"  [FAIL ] {ref:20s} 拉取失败，保留原快照")
            continue
        total, added, removed = stat
        status = "有变更" if added or removed else "无变化"
        changed = changed or bool(added or removed)
        rows.append(f"| {ref} | {total} | +{added} | -{removed} | {status} |")
        print(f"  [{'DIFF ' if added or removed else 'SAME '}] {ref:20s} 共 {total:7d} 条  +{added:<6d} -{removed:<6d}")

    print(f"\n[*] CDN 快照{'有变更' if changed else '无变化'}" + (f"，{len(failed)} 个拉取失败" if failed else ""))

    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as f:
            f.write("## Gins-Rules CDN 快照\n\n| 规则集 | 条数 | 新增 | 删除 | 状态 |\n|---|---|---|---|---|\n")
            f.write("\n".join(rows) + "\n")
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as f:
            f.write(f"changed={'true' if changed else 'false'}\n")

    return 1 if len(failed) == len(refs) else 0


if __name__ == "__main__":
    sys.exit(main())
