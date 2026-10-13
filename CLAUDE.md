# CLAUDE.md — Proxy Rulesets 项目维护规范

## 项目定位
多平台规则集编译仓库，将 DustinWin、MetaCubeX、echs-top、GinsRule-git 等上游分散规则及自定义白名单解构、合并、去重，统一编译并导出为 4 大核心流向规则：
- `direct` (走直连)
- `apac` (走亚太)
- `ai` (走 AI)
- `lowrate` (走低倍)

## 目标平台
1. **Mihomo**: `.mrs` (二进制) + `.yaml` (源文本)
2. **Loon**: `.lsr`
3. **Shadowrocket**: `.list`
4. **Sing-box**: `.srs` (二进制) + `.json` (源文本)

## 目录结构
```text
proxy-rulesets/
├── .github/workflows/build-rulesets.yml   # 自动编译与发版流水线
├── custom/                                # 本地自定义规则 (解构自 mihomo_Region.yaml)
│   ├── direct_custom.txt
│   ├── apac_custom.txt
│   ├── ai_custom.txt
│   └── lowrate_custom.txt
├── scripts/
│   ├── config.py                          # 数据源 URL 与分类定义
│   ├── parser.py                          # 跨格式规则解析器
│   ├── build.py                           # 抓取、去重与导出主脚本
│   └── compile.sh                         # 调用 mihomo 与 sing-box 编译二进制
└── rules/                                 # 编译输出目录
    ├── mihomo/
    ├── loon/
    ├── shadowrocket/
    └── singbox/
```

## 冲突判定与优先级规范
跨分类去重严格遵循：
`direct` (直连白名单最高优先级) -> `apac` (即时通讯优先保障) -> `ai` (AI 与核心服务) -> `lowrate` (流媒体及兜底大盘)

## 构建与运行
```bash
python3 scripts/build.py
./scripts/compile.sh
```
