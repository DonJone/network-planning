#!/bin/bash
set -e

BASE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
MIHOMO_BIN="${BASE_DIR}/bin/mihomo"
SINGBOX_BIN="${BASE_DIR}/bin/sing-box"

if [ ! -f "$MIHOMO_BIN" ]; then
    MIHOMO_BIN="$(which mihomo 2>/dev/null || true)"
fi
if [ ! -f "$SINGBOX_BIN" ]; then
    SINGBOX_BIN="$(which sing-box 2>/dev/null || true)"
fi

if [ -z "$MIHOMO_BIN" ] || [ ! -f "$MIHOMO_BIN" ]; then
    echo "[ERROR] 未找到 mihomo 编译器二进制！"
    exit 1
fi
if [ -z "$SINGBOX_BIN" ] || [ ! -f "$SINGBOX_BIN" ]; then
    echo "[ERROR] 未找到 sing-box 编译器二进制！"
    exit 1
fi

echo "[*] 开始编译 Mihomo 二进制规则集 (.mrs)..."
for cat in direct apac ai lowrate; do
    echo "  -> 编译 Mihomo: ${cat}.mrs (domain)"
    "$MIHOMO_BIN" convert-ruleset domain yaml "${BASE_DIR}/rules/mihomo/${cat}.yaml" "${BASE_DIR}/rules/mihomo/${cat}.mrs"
    
    if [ -f "${BASE_DIR}/rules/mihomo/${cat}_ip.yaml" ]; then
        echo "  -> 编译 Mihomo: ${cat}_ip.mrs (ipcidr)"
        "$MIHOMO_BIN" convert-ruleset ipcidr yaml "${BASE_DIR}/rules/mihomo/${cat}_ip.yaml" "${BASE_DIR}/rules/mihomo/${cat}_ip.mrs"
    fi
done

echo "[*] 开始编译 Sing-box 二进制规则集 (.srs)..."
for cat in direct apac ai lowrate; do
    echo "  -> 编译 Sing-box: ${cat}.srs"
    "$SINGBOX_BIN" rule-set compile "${BASE_DIR}/rules/singbox/${cat}.json" -o "${BASE_DIR}/rules/singbox/${cat}.srs"
done

echo "[*] 全部二进制规则编译完成！"
