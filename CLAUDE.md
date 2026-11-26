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

例外：`src/config.rs` 中 `override_direct` 来源与 `OVERRIDE_DIRECT_PATTERNS` 正则命中的域名（AWS 国际站与中国区、AWS Quick）强制归入 `ai`，不受 direct/apac 优先级吞并；同时将公共第三方 CDN 后缀（如 `cloudfront.net`）及 GitHub/流媒体域名移出 `ai`，避免第三方大流量走 AI 节点。

跨平台父域剔除与优先覆盖：被 `ai` 父级后缀或关键字覆盖、但实际归属 `lowrate` 的域名（含 `lowrate_custom` 相册/网盘/YouTube API、流媒体专用源、S3/ELB/GCS 等多租户云存储子域及关键字误杀项，严格跳过裸 AWS 区域根域及真实 AI 子域）：
- Sing-box `ai.srs` 内置 `AND NOT` 逻辑剔除，客户端无需调整配置；
- 同步导出极小优先规则集 `lowrate_override` (`.mrs` / `.lsr` / `.list` / `.srs`)，供 Mihomo / Loon / Shadowrocket 置于 `ai` 之前指向 `低倍`。

## 构建保护、校验与发布回滚
- **拉取回退与降幅守卫**：jsDelivr 主地址拉取失败自动回退另一节点与 `raw.githubusercontent.com`；任意远程源最终失败/解析 0 条，或相对基线 `rules/manifest.json` 出现单源/分类条数下降超 30%（可通过 `--max-drop` / `--allow-drop` 调整）时，`build` 返回非零阻断发布。
- **CI 逆校验门禁**：`verify` 校验 Sing-box 反编译域名与合并后 CIDR 精确等价、按优先级检验 Sing-box 与 Mihomo 离线内核分流命中性，任一失败返回非零阻断发布。
- **增量发布与标签回滚**：`release` 分支采用 worktree 增量提交保留历史，每次变更打 `rules-YYYYMMDD-HHMM` 标签（保留最近 14 个），支持在 Actions 通过 `rollback_tag` 一键回滚。

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
