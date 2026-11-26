# CLAUDE.md — Proxy Rulesets 项目维护规范

## 项目定位
多平台规则集编译仓库，将 DustinWin、MetaCubeX、echs-top 等上游分散规则、内置 Gins-Rules 源 (`gins-rules/`，以 git subtree 并入) 及自定义白名单解构、合并、去重，统一编译并导出为 4 大核心流向规则：
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
network-planning/
├── .github/workflows/build-rulesets.yml   # 自动编译与发版流水线
├── gins-rules/                            # 内置 Gins-Rules (git subtree，原 DonJone/Gins-Rules)
│   ├── source/                            # 规则源文本，build 命令通过 src/config.rs 的 gins 字段直接读取
│   ├── cdn/shadowrocket/                  # rules.ichimarugin728.dev 每日快照 (sync-cdn 命令生成)
│   └── infra/aggregator/                  # Rust rulekit：同步上游 (sync) 与规范化 (normalize)
├── custom/                                # 本地自定义规则 (解构自 mihomo_Region.yaml)
│   ├── direct_custom.txt
│   ├── apac_custom.txt
│   ├── ai_custom.txt
│   └── lowrate_custom.txt
├── Cargo.toml                             # Rust 构建工具 network-planning
├── src/
│   ├── main.rs                            # CLI 入口：sync-cdn / build / compile / verify / all
│   ├── config.rs                          # 数据源 URL 与分类定义
│   ├── parser.rs                          # 跨格式规则解析器
│   ├── builder.rs                         # 抓取、冲突判定与去重主流程
│   ├── export.rs                          # 四大平台规则文本导出
│   ├── sync_cdn.rs                        # Gins CDN 快照同步
│   ├── compile.rs                         # 调用 mihomo 与 sing-box 编译二进制
│   └── verify.rs                          # .mrs / .srs 逆校验套件
└── rules/                                 # 编译输出目录
    ├── mihomo/
    ├── loon/
    ├── shadowrocket/
    └── singbox/
```

## 冲突判定与优先级规范
跨分类去重严格遵循：
`direct` (直连白名单最高优先级) -> `apac` (即时通讯优先保障) -> `ai` (AI 与核心服务) -> `lowrate` (流媒体及兜底大盘)

例外：`src/config.rs` 中 `override_direct` 来源与 `OVERRIDE_DIRECT_PATTERNS` 正则命中的域名（AWS 国际站与中国区、AWS Quick）强制归入 `ai`，不受 direct/apac 优先级吞并。

## 构建与运行
```bash
# 可选：同步 Gins-Rules 上游源
(cd gins-rules && cargo run --manifest-path infra/aggregator/Cargo.toml --release -- sync)
# 可选：拉取 Gins CDN 快照并查看变更
cargo build --release
./target/release/network-planning sync-cdn
./target/release/network-planning build
./target/release/network-planning compile   # 需要 bin/ 或 PATH 中的 mihomo、sing-box
# 可选：逆校验二进制规则
./target/release/network-planning verify
```

## Gins-Rules 子树维护
- `src/config.rs` 中 `gins` 字段格式为 `{分类}/{规则名}`，合并读取 `gins-rules/source/{分类}/`、`gins-rules/source/upstream/{分类}/` 与 CDN 快照 `gins-rules/cdn/shadowrocket/{分类}/`，`asn` 分类对应 `ip/`；非 `proxy` 分类按 Gins 编译器 `sanitizeRules` 剔除强制代理域名。
- CI 每日运行 `network-planning sync-cdn` 拉取 CDN，归一化排序后仅在内容变化时写入快照并自动提交，变更统计输出到 Actions Job Summary；拉取失败保留旧快照。
- 直接在 `gins-rules/` 内修改并随本仓库提交；如需与原仓库同步：`git subtree pull --prefix=gins-rules https://github.com/DonJone/Gins-Rules.git main`。
