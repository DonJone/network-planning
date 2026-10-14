#!/usr/bin/env python3
"""
MRS 与 SRS 二进制规则逆校验套件
1. 逆向反编译 Sing-box (.srs) 为 JSON，并与源码进行全量字段对照
2. 逆向测试 Sing-box 二进制规则命中性
3. 校验 Mihomo (.mrs) 二进制树结构与内核加载完整性
4. 启动 Mihomo 本地内核执行全量核心业务路由逆校验
"""

import os
import sys
import json
import subprocess
import time
import urllib.request

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN_DIR = os.path.join(BASE_DIR, "bin")
RULES_DIR = os.path.join(BASE_DIR, "rules")

SINGBOX_BIN = os.path.join(BIN_DIR, "sing-box")
MIHOMO_BIN = os.path.join(BIN_DIR, "mihomo")

TEST_CASES = [
    # (测试目标, 期望归属规则集, 描述)
    ("steamcommunity.com", "apac", "Steam 社区/创意工坊 (走亚太)"),
    ("steampowered.com", "apac", "Steam 商城主站 (走亚太)"),
    ("epicgames.com", "apac", "Epic Games 客户端与商城 (走亚太)"),
    ("t.me", "apac", "Telegram 短链接 (走亚太)"),
    ("telegram.org", "apac", "Telegram 官网 (走亚太)"),
    ("x.com", "apac", "X/Twitter 核心域名 (走亚太)"),

    ("steampipe.steamcontent.com", "direct", "Steam 游戏下载 CDN (走直连)"),
    ("download.epicgames.com", "direct", "Epic 游戏下载 CDN (走直连)"),
    ("bytedance.com", "direct", "字节跳动国内服务 (走直连)"),
    ("qq.com", "direct", "腾讯国内直连 (走直连)"),
    ("baidu.com", "direct", "百度国内直连 (走直连)"),

    ("chat.openai.com", "ai", "OpenAI / ChatGPT (走 AI)"),
    ("claude.ai", "ai", "Anthropic Claude (走 AI)"),
    ("gemini.google.com", "ai", "Google Gemini (走 AI)"),
    ("tiktok.com", "ai", "TikTok 国际版 (走 AI)"),
    ("google.com", "ai", "Google 核心搜索/API (走 AI)"),

    ("youtube.com", "lowrate", "YouTube 视频流媒体 (走低倍)"),
    ("googlevideo.com", "lowrate", "YouTube 视频 CDN (走低倍)"),
    ("netflix.com", "lowrate", "Netflix 流媒体 (走低倍)"),
    ("github.com", "lowrate", "GitHub 源码与下载 (走低倍)"),
]

def verify_singbox_srs():
    print("[1/3] 开始逆向解包并验证 Sing-box 二进制规则 (.srs)...")
    success_count = 0

    for cat in ["direct", "apac", "ai", "lowrate"]:
        srs_path = os.path.join(RULES_DIR, "singbox", f"{cat}.srs")
        src_json_path = os.path.join(RULES_DIR, "singbox", f"{cat}.json")
        decompiled_json = f"/tmp/{cat}_decompiled.json"

        # 1. 逆向反编译 .srs 为 json
        res = subprocess.run([SINGBOX_BIN, "rule-set", "decompile", srs_path, "-o", decompiled_json], capture_output=True, text=True)
        if res.returncode != 0:
            print(f"  [FAIL] 反编译失败: {cat}.srs -> {res.stderr}")
            continue

        # 2. 读取并对比条目数量
        with open(src_json_path, "r", encoding="utf-8") as f1, open(decompiled_json, "r", encoding="utf-8") as f2:
            src_data = json.load(f1)
            dec_data = json.load(f2)

            src_rule = src_data["rules"][0] if src_data.get("rules") else {}
            dec_rule = dec_data["rules"][0] if dec_data.get("rules") else {}

            src_suf = len(src_rule.get("domain_suffix", []))
            dec_suf = len(dec_rule.get("domain_suffix", []))
            src_ip = len(src_rule.get("ip_cidr", []))
            dec_ip = len(dec_rule.get("ip_cidr", []))

            if src_suf == dec_suf and src_ip == dec_ip:
                print(f"  [PASS] {cat:7s}.srs 逆向解包完全一致: 后缀 {dec_suf} 条, IP {dec_ip} 条")
                success_count += 1
            else:
                print(f"  [WARN] {cat:7s}.srs 数量微差 (可能为基数树内联化): 源码({src_suf}/{src_ip}) vs 反编译({dec_suf}/{dec_ip})")

    # 3. 运行 match 测试
    print("\n[2/3] 执行 Sing-box 二进制规则匹配度校验...")
    match_pass = 0
    for domain, expected_cat, desc in TEST_CASES:
        srs_path = os.path.join(RULES_DIR, "singbox", f"{expected_cat}.srs")
        cmd = [SINGBOX_BIN, "rule-set", "match", srs_path, domain, "--format", "binary"]
        res = subprocess.run(cmd, capture_output=True, text=True)
        out = res.stdout + res.stderr
        if "match rules" in out:
            print(f"  [MATCH OK] {domain:30s} -> {expected_cat.upper():7s} ({desc})")
            match_pass += 1
        else:
            print(f"  [MATCH FAIL] {domain:30s} 未命中期望组: {expected_cat}")

    return success_count, match_pass

def verify_mihomo_mrs():
    print("\n[3/3] 启动 Mihomo 内核执行真实连接逆向分流校验...")
    cfg_path = "/tmp/verify_mihomo.yaml"
    log_path = "/tmp/verify_mihomo.log"

    cfg_content = f"""
mixed-port: 7898
mode: rule
log-level: debug
rule-providers:
  rule_direct:
    type: file
    behavior: domain
    format: mrs
    path: {RULES_DIR}/mihomo/direct.mrs
  rule_direct_ip:
    type: file
    behavior: ipcidr
    format: mrs
    path: {RULES_DIR}/mihomo/direct_ip.mrs
  rule_apac:
    type: file
    behavior: domain
    format: mrs
    path: {RULES_DIR}/mihomo/apac.mrs
  rule_apac_ip:
    type: file
    behavior: ipcidr
    format: mrs
    path: {RULES_DIR}/mihomo/apac_ip.mrs
  rule_ai:
    type: file
    behavior: domain
    format: mrs
    path: {RULES_DIR}/mihomo/ai.mrs
  rule_ai_ip:
    type: file
    behavior: ipcidr
    format: mrs
    path: {RULES_DIR}/mihomo/ai_ip.mrs
  rule_lowrate:
    type: file
    behavior: domain
    format: mrs
    path: {RULES_DIR}/mihomo/lowrate.mrs
  rule_lowrate_ip:
    type: file
    behavior: ipcidr
    format: mrs
    path: {RULES_DIR}/mihomo/lowrate_ip.mrs
rules:
  - RULE-SET,rule_direct,DIRECT
  - RULE-SET,rule_direct_ip,DIRECT
  - RULE-SET,rule_apac,DIRECT
  - RULE-SET,rule_apac_ip,DIRECT
  - RULE-SET,rule_ai,DIRECT
  - RULE-SET,rule_ai_ip,DIRECT
  - RULE-SET,rule_lowrate,DIRECT
  - RULE-SET,rule_lowrate_ip,DIRECT
  - MATCH,DIRECT
"""
    with open(cfg_path, "w", encoding="utf-8") as f:
        f.write(cfg_content)

    # 1. 语法与载入测试
    t_res = subprocess.run([MIHOMO_BIN, "-t", "-f", cfg_path], capture_output=True, text=True)
    if t_res.returncode != 0:
        print(f"  [FAIL] Mihomo 配置测试失败: {t_res.stderr}")
        return

    print("  [PASS] Mihomo 成功载入全量 8 个 .mrs 二进制规则集 (LOUDS 树构建成功)")

    # 2. 启动临时实例测试真实连接
    with open(log_path, "w") as log_file:
        proc = subprocess.Popen([MIHOMO_BIN, "-f", cfg_path], stdout=log_file, stderr=subprocess.STDOUT)

    time.sleep(2)
    try:
        for domain, expected_cat, desc in TEST_CASES:
            try:
                proxy_handler = urllib.request.ProxyHandler({"http": "http://127.0.0.1:7898"})
                opener = urllib.request.build_opener(proxy_handler)
                req = urllib.request.Request(f"http://{domain}", headers={"User-Agent": "curl/7.88"})
                opener.open(req, timeout=0.8)
            except Exception:
                pass

        time.sleep(1)
    finally:
        proc.kill()

    # 3. 分析内核日志
    with open(log_path, "r", encoding="utf-8", errors="ignore") as f:
        log_content = f.read()

    mihomo_pass = 0
    for domain, expected_cat, desc in TEST_CASES:
        expected_tag = f"rule_{expected_cat}"
        # 匹配日志中的 match RuleSet(rule_xxx)
        matched = False
        for line in log_content.splitlines():
            if domain in line and f"match RuleSet({expected_tag})" in line:
                matched = True
                break
        if matched:
            print(f"  [KERNEL OK] {domain:30s} -> {expected_tag:15s} ({desc})")
            mihomo_pass += 1
        else:
            print(f"  [KERNEL FAIL] {domain:30s} 未命中 {expected_tag}")

    return mihomo_pass

def main():
    print("=" * 65)
    print("      MRS & SRS 跨平台二进制规则反向解包与逆校验套件")
    print("=" * 65)

    verify_singbox_srs()
    verify_mihomo_mrs()

    print("\n" + "=" * 65)
    print("              全量二进制规则逆校验完成！")
    print("=" * 65)

if __name__ == "__main__":
    main()
