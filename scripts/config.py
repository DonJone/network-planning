#!/usr/bin/env python3
"""
数据源配置文件
严格按照用户默认选择顺序将全部上游规则与自定义规则划分为 4 大块：
1. direct   (走直连: 原生直连 + 游戏平台 + 微软苹果Nvidia)
2. apac     (走亚太: 通讯 Telegram, X)
3. ai       (走 AI: AI与Google + TikTok + AWS/AWS Quick)
4. lowrate  (走低倍: Emby流媒体Github + 兜底)
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
            # Gins-Rules (原 GinsRule-git，现由 R2/Worker 分发)
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/direct/direct-list.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/steam.list",
        ],
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
            # Gins-Rules (原 GinsRule-git，现由 R2/Worker 分发)
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/telegram.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/twitter.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/ip/telegram.list",
        ],
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
            # Gins-Rules (原 GinsRule-git，现由 R2/Worker 分发)
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/ai-other.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/ip/asn-google.list",
        ],
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
            # Gins-Rules (原 GinsRule-git，现由 R2/Worker 分发)
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/github.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/netflix.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/disney.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/hbo.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/spotify.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/appletv.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/proxy-list.list",
            "https://rules.ichimarugin728.dev/ruleset/shadowrocket/proxy/gfw-list.list",
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
