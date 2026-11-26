#!/usr/bin/env python3
"""
数据源配置文件
严格按照用户默认选择顺序将全部上游规则与自定义规则划分为 4 大块：
1. direct   (走直连: 原生直连 + 游戏平台 + 微软苹果Nvidia)
2. apac     (走亚太: 通讯 Telegram, X)
3. ai       (走 AI: AI与Google + TikTok + AWS/AWS Quick)
4. lowrate  (走低倍: Emby流媒体Github + 兜底)

每个分类的来源字段:
- custom: 本地自定义规则文件
- remote: 远程上游规则 URL
- gins:   内置 Gins-Rules 源 (gins-rules/source)，格式为 "{分类}/{规则名}"，
          读取 source/{分类}/{规则名}.txt 与 source/upstream/{分类}/{规则名}.txt 并合并，
          asn 分类对应 source/ip 目录；同时合并 gins-rules/cdn/ 下的 CDN 每日快照
          (由 scripts/sync_gins_cdn.py 从 rules.ichimarugin728.dev 拉取)
"""

SOURCES = {
    "direct": {
        "name": "走直连",
        "description": "国内大盘、局域网私有地址、Apple/Microsoft国内及海外生态、国内游戏加速与核心直连服务",
        "custom": ["custom/direct_custom.txt"],
        "remote": [
            # DustinWin
            "https://github.com/DustinWin/ruleset_geodata/releases/download/mihomo-ruleset/private.list",
            "https://github.com/DustinWin/ruleset_geodata/releases/download/mihomo-ruleset/privateip.list",
            # echs-top
            "https://fastly.jsdelivr.net/gh/echs-top/proxy@main/list/domain/cn.list",
            "https://fastly.jsdelivr.net/gh/echs-top/proxy@main/list/domain/dnsmasq-china.list",
            "https://fastly.jsdelivr.net/gh/echs-top/proxy@main/list/ip/cn.list",
            # MetaCubeX
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/apple@cn.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/microsoft.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/microsoft@cn.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/category-games@cn.list",
        ],
        "gins": ["direct/direct-list", "proxy/steam"],
    },
    "apac": {
        "name": "走亚太",
        "description": "Telegram 全量通信与纯 IP 网段、X (Twitter) 等低延迟海外通讯业务",
        "custom": ["custom/apac_custom.txt"],
        "remote": [
            # MetaCubeX
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/telegram.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/x.list",
            # echs-top
            "https://fastly.jsdelivr.net/gh/echs-top/proxy@main/list/ip/telegram.list",
        ],
        "gins": ["proxy/telegram", "proxy/twitter", "ip/telegram"],
    },
    "ai": {
        "name": "走 AI",
        "description": "OpenAI, Claude, Gemini, Copilot 等 AI 模型与 Google 全家桶、FCM 推送、TikTok 业务，以及 AWS 控制台/机器申请与 AWS Quick 服务",
        "custom": ["custom/ai_custom.txt"],
        "remote": [
            # DustinWin
            "https://github.com/DustinWin/ruleset_geodata/releases/download/mihomo-ruleset/ai.list",
            # MetaCubeX
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/google.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/google@cn.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/googlefcm.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/tiktok.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geoip/google.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/aws.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/aws@cn.list",
        ],
        "gins": ["proxy/ai-other", "asn/asn-google"],
        # 以下来源的域名优先级高于 direct：AWS 国际站与中国区均强制走 AI，不允许被国内直连大盘吞掉
        "override_direct": [
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/aws.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/aws@cn.list",
        ],
    },
    "lowrate": {
        "name": "走低倍",
        "description": "YouTube, Netflix, Disney+, Emby 全量影视服、GitHub 源码下载、通用代理兜底与大流量吞吐业务",
        "custom": ["custom/lowrate_custom.txt"],
        "remote": [
            # MetaCubeX
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/apple.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/youtube.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/hcaptcha.list",
            "https://fastly.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/geolocation-!cn.list",
            # DustinWin
            "https://github.com/DustinWin/ruleset_geodata/releases/download/mihomo-ruleset/media.list",
            "https://github.com/DustinWin/ruleset_geodata/releases/download/mihomo-ruleset/mediaip.list",
            "https://github.com/DustinWin/ruleset_geodata/releases/download/mihomo-ruleset/trackerslist.list",
            # echs-top
            "https://fastly.jsdelivr.net/gh/echs-top/proxy@main/list/domain/proxy.list",
            "https://fastly.jsdelivr.net/gh/echs-top/proxy@main/list/domain/proxy-ltsc.list",
        ],
        "gins": [
            "proxy/github",
            "proxy/netflix",
            "proxy/disney",
            "proxy/hbo",
            "proxy/spotify",
            "proxy/appletv",
            "proxy/proxy-list",
            "proxy/gfw-list",
        ],
    },
}

# 强制分流的正则补充 (匹配无法通过上游 list 枚举的域名)，格式: (正则, 目标分类)
# 命中的域名不会落入 direct / apac，用于保障 AWS 中国区解析与 CDN 走 AI 组
OVERRIDE_DIRECT_PATTERNS = [
    (r"^awsdns-cn-\d+\.(com|net|org|biz|cn)$", "ai"),
    (r"(^|\.)amzndns-cn\.[a-z.]+$", "ai"),
    (r"(^|\.)cloudfront-cn\.net$", "ai"),
]

# Gins-Rules CDN 根路径 (Shadowrocket 格式)，由 scripts/sync_gins_cdn.py 每日拉取快照
GINS_CDN_BASE = "https://rules.ichimarugin728.dev/ruleset/shadowrocket"
