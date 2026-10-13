# Proxy Rulesets (四大核心流量分流规则集)

自动化全量逆向、解构、合并、去重并编译的多平台规则集工程。

严格按照默认出口策略将全网规则归纳收敛为 **4 大方向**，一次构建并全量分发至 **Mihomo (Clash Meta)、Loon、Shadowrocket、Sing-box** 四大平台。

---

## 架构理念：规则分类与出口策略 1:1 强绑定

不再在客户端维护数十个碎片化的规则集，而是直接按出口策略分流：

| 规则大块 | 策略定位 | 核心覆盖范围与规则源 |
| :--- | :--- | :--- |
| **direct** | 走直连 (`DIRECT`) | 国内域名大盘 (`cn`, `dnsmasq-china`)、局域网私有地址 (`private`)、国内 IP (`cn_ip`)、Apple/Microsoft 国内生态与直连、国内游戏平台 (`steam`, `games-cn`)、字节跳动/抖音等核心服务 |
| **apac** | 走亚太 (`亚太`) | Telegram 全量通信与纯 IP 网段 (避让 QUIC)、X (Twitter) 等低延迟海外即时通讯业务 |
| **ai** | 走 AI (`AI`) | OpenAI、Claude、Gemini、Copilot、Perplexity 等高敏感 AI 模型，以及 Google 全家桶、FCM 推送、TikTok 业务 |
| **lowrate** | 走低倍 (`低倍`) | YouTube、Netflix、Disney+、Emby 全量影视服域名与关键字、GitHub 源码与大文件、PT Tracker、通用海外代理兜底 (`proxy`, `geolocation-!cn`) |

---

## 产出矩阵与订阅链接

构建产物发布于本仓库的 `release` 分支，支持通过 jsDelivr CDN 高速引用：

### 1. Mihomo / Clash Meta (`.mrs` 二进制与 `.yaml` 文本)

| 分类 | 域名规则 (Behavior: Domain) | IP 规则 (Behavior: IPCIDR) |
| :--- | :--- | :--- |
| **走直连** | [direct.mrs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/direct.mrs) | [direct_ip.mrs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/direct_ip.mrs) |
| **走亚太** | [apac.mrs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/apac.mrs) | [apac_ip.mrs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/apac_ip.mrs) |
| **走 AI** | [ai.mrs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/ai.mrs) | [ai_ip.mrs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/ai_ip.mrs) |
| **走低倍** | [lowrate.mrs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/lowrate.mrs) | [lowrate_ip.mrs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/lowrate_ip.mrs) |

#### Mihomo 极简配置示例：

```yaml
rule-providers:
  rule-direct: { type: http, interval: 86400, behavior: domain, format: mrs, url: "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/direct.mrs", path: ./rules/direct.mrs }
  rule-direct-ip: { type: http, interval: 86400, behavior: ipcidr, format: mrs, url: "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/direct_ip.mrs", path: ./rules/direct_ip.mrs }
  rule-apac: { type: http, interval: 86400, behavior: domain, format: mrs, url: "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/apac.mrs", path: ./rules/apac.mrs }
  rule-apac-ip: { type: http, interval: 86400, behavior: ipcidr, format: mrs, url: "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/apac_ip.mrs", path: ./rules/apac_ip.mrs }
  rule-ai: { type: http, interval: 86400, behavior: domain, format: mrs, url: "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/ai.mrs", path: ./rules/ai.mrs }
  rule-ai-ip: { type: http, interval: 86400, behavior: ipcidr, format: mrs, url: "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/ai_ip.mrs", path: ./rules/ai_ip.mrs }
  rule-lowrate: { type: http, interval: 86400, behavior: domain, format: mrs, url: "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/lowrate.mrs", path: ./rules/lowrate.mrs }
  rule-lowrate-ip: { type: http, interval: 86400, behavior: ipcidr, format: mrs, url: "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/mihomo/lowrate_ip.mrs", path: ./rules/lowrate_ip.mrs }

rules:
  - RULE-SET,rule-direct,DIRECT
  - RULE-SET,rule-direct-ip,DIRECT,no-resolve
  - RULE-SET,rule-apac,亚太
  - RULE-SET,rule-apac-ip,亚太,no-resolve
  - RULE-SET,rule-ai,AI
  - RULE-SET,rule-ai-ip,AI,no-resolve
  - RULE-SET,rule-lowrate,低倍
  - RULE-SET,rule-lowrate-ip,低倍,no-resolve
  - MATCH,低倍
```

---

### 2. Loon (`.lsr` 远程规则)

| 分类 | 订阅链接 |
| :--- | :--- |
| **走直连** | [direct.lsr](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/loon/direct.lsr) |
| **走亚太** | [apac.lsr](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/loon/apac.lsr) |
| **走 AI** | [ai.lsr](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/loon/ai.lsr) |
| **走低倍** | [lowrate.lsr](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/loon/lowrate.lsr) |

#### Loon 配置示例：

```ini
[Remote Rule]
https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/loon/direct.lsr, policy=DIRECT, tag=走直连, enabled=true
https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/loon/apac.lsr, policy=亚太, tag=走亚太, enabled=true
https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/loon/ai.lsr, policy=AI, tag=走AI, enabled=true
https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/loon/lowrate.lsr, policy=低倍, tag=走低倍, enabled=true
```

---

### 3. Shadowrocket (`.list` 规则集)

| 分类 | 订阅链接 |
| :--- | :--- |
| **走直连** | [direct.list](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/shadowrocket/direct.list) |
| **走亚太** | [apac.list](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/shadowrocket/apac.list) |
| **走 AI** | [ai.list](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/shadowrocket/ai.list) |
| **走低倍** | [lowrate.list](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/shadowrocket/lowrate.list) |

#### Shadowrocket 配置示例：

```ini
[Rule]
RULE-SET,https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/shadowrocket/direct.list,DIRECT
RULE-SET,https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/shadowrocket/apac.list,亚太
RULE-SET,https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/shadowrocket/ai.list,AI
RULE-SET,https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/shadowrocket/lowrate.list,低倍
FINAL,低倍
```

---

### 4. Sing-box (`.srs` 二进制与 `.json` 规则集)

| 分类 | 二进制格式 (`.srs`) | JSON 源格式 (`.json`) |
| :--- | :--- | :--- |
| **走直连** | [direct.srs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/direct.srs) | [direct.json](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/direct.json) |
| **走亚太** | [apac.srs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/apac.srs) | [apac.json](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/apac.json) |
| **走 AI** | [ai.srs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/ai.srs) | [ai.json](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/ai.json) |
| **走低倍** | [lowrate.srs](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/lowrate.srs) | [lowrate.json](https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/lowrate.json) |

#### Sing-box 配置示例：

```json
{
  "route": {
    "rule_set": [
      { "tag": "rule-direct", "type": "remote", "format": "binary", "url": "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/direct.srs", "download_detour": "direct" },
      { "tag": "rule-apac", "type": "remote", "format": "binary", "url": "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/apac.srs", "download_detour": "proxy" },
      { "tag": "rule-ai", "type": "remote", "format": "binary", "url": "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/ai.srs", "download_detour": "proxy" },
      { "tag": "rule-lowrate", "type": "remote", "format": "binary", "url": "https://fastly.jsdelivr.net/gh/DonJone/proxy-rulesets@release/rules/singbox/lowrate.srs", "download_detour": "proxy" }
    ],
    "rules": [
      { "rule_set": "rule-direct", "outbound": "direct" },
      { "rule_set": "rule-apac", "outbound": "亚太" },
      { "rule_set": "rule-ai", "outbound": "AI" },
      { "rule_set": "rule-lowrate", "outbound": "低倍" }
    ],
    "final": "低倍"
  }
}
```

---

## 本地开发与构建

```bash
# 1. 抓取上游数据源并清洗导出跨生态规则文本
python3 scripts/build.py

# 2. 编译 Mihomo (.mrs) 与 Sing-box (.srs) 二进制文件
./scripts/compile.sh
```
