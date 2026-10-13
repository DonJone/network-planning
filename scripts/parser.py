#!/usr/bin/env python3
"""
统一规则解析器
将不同来源（Clash/Mihomo、Loon、Shadowrocket、Sing-box、MetaCubeX）的文本规则归一化为标准内部结构。
"""

import re
import ipaddress

class RuleParser:
    @staticmethod
    def parse_line(line: str):
        line = line.strip()
        if not line or line.startswith(("#", "//", ";")):
            return None

        # 去除行内注释和多余标记 (如 ,no-resolve, ,DIRECT 等策略标识)
        parts = [p.strip() for p in line.split(",") if p.strip()]
        if not parts:
            return None

        first = parts[0]
        first_upper = first.upper()

        # 1. 标准 Shadowrocket / Loon / Clash 前缀
        if first_upper in ("DOMAIN-SUFFIX", "DOMAIN-KEYWORD", "DOMAIN", "IP-CIDR", "IP-CIDR6"):
            if len(parts) >= 2:
                rtype = first_upper
                val = parts[1].strip().lower()
                # 校验 IP
                if rtype in ("IP-CIDR", "IP-CIDR6"):
                    return ("IP-CIDR", val)
                return (rtype, val)

        # 2. MetaCubeX / V2Ray 常用前缀
        if first.startswith("+.") or first.startswith("."):
            domain = first.lstrip("+.").lower()
            if domain:
                return ("DOMAIN-SUFFIX", domain)
        elif first.startswith("+"):
            domain = first.lstrip("+").lower()
            if domain:
                return ("DOMAIN-SUFFIX", domain)
        elif first.startswith("full:"):
            domain = first[5:].lower()
            if domain:
                return ("DOMAIN", domain)
        elif first.startswith("keyword:"):
            kw = first[8:].lower()
            if kw:
                return ("DOMAIN-KEYWORD", kw)

        # 3. 判断是否为纯 IP-CIDR 或 单个 IP
        try:
            # 尝试解析为 IP / CIDR
            ip_obj = ipaddress.ip_network(first, strict=False)
            return ("IP-CIDR", str(ip_obj))
        except ValueError:
            pass

        # 4. 判断是否为有效域名
        # 如果看起来像域名（包含至少一个点，且不含非法字符）
        domain_candidate = first.lower()
        if re.match(r"^[a-z0-9_\-\.]+\.[a-z0-9_\-]+$", domain_candidate):
            return ("DOMAIN-SUFFIX", domain_candidate)

        return None
