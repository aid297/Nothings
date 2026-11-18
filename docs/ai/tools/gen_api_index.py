#!/usr/bin/env python3
"""Generate docs/ai/open-api/public-api.json from the Rust sources.

Only the Python standard library is used. The source tree is the single source
of truth: every public item is emitted with its file and line number so an AI
assistant (or a human) can jump straight to the implementation.

Usage:
    python3 docs/ai/tools/gen_api_index.py            # regenerate the JSON
    python3 docs/ai/tools/gen_api_index.py --check    # fail if JSON is stale
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "docs" / "ai" / "open-api" / "public-api.json"

# (源根, crate 模块前缀)。四个 crate 各自独立，扫描器统一汇总。
SOURCE_ROOTS: list[tuple[Path, str]] = [
    (ROOT / "src", "nothings"),
    (ROOT / "nothings-orm" / "src", "nothings_orm"),
    (ROOT / "nothings-orm-sqlite" / "src", "nothings_orm_sqlite"),
    (ROOT / "nothings-orm-mysql" / "src", "nothings_orm_mysql"),
]
SRC = SOURCE_ROOTS[0][0]  # 兼容旧引用；新逻辑一律走 SOURCE_ROOTS

MANIFESTS: list[tuple[str, Path]] = [
    ("nothings", ROOT / "Cargo.toml"),
    ("nothings-orm", ROOT / "nothings-orm" / "Cargo.toml"),
    ("nothings-orm-sqlite", ROOT / "nothings-orm-sqlite" / "Cargo.toml"),
    ("nothings-orm-mysql", ROOT / "nothings-orm-mysql" / "Cargo.toml"),
]

SPEC_VERSION = "nothings-public-api/v1"
GENERATOR_VERSION = 3
# Not in Cargo.toml: determined by the `aes 0.9` dependency (rust-version 1.89).
# mysql 28.0.4 / mysql_common 0.37.3 未声明 rust-version；已用 cargo +1.89.0 check 实测通过，见 spec/51。
MSRV = "1.89+"

# Key is "Owner::name" (for methods/fields) or "name" (for free items).
# status: ok | broken | limited
# "limited" = works, but has a documented trap; "broken" = do not rely on it.
ANNOTATIONS: dict[str, tuple[str, str]] = {
    "AnySlice::remove_by_indexes": ("broken", "A1: 惰性迭代器未消费，实际为空操作；连带使 AnyMap 的四个删除/过滤方法失效"),
    "AnySlice::shuffle": ("broken", "A3: 打乱的是临时克隆，返回原顺序；改用 shuffle_self"),
    "AnySlice::get_indexes_by_values": ("limited", "B1: 内部 unwrap，任一值不存在即 panic"),
    "AnySlice::set_value": ("limited", "B2: 索引越界会 panic"),
    "AnySlice::chunk": ("limited", "B2: size == 0 会 panic"),
    "AnyMap::filter": ("broken", "A2: 依赖 AnySlice::remove_by_indexes，调用后数据不变"),
    "AnyMap::remove_by_keys": ("broken", "A2: 同上"),
    "AnyMap::remove_by_values": ("broken", "A2: 同上"),
    "AnyMap::remove_empty": ("broken", "A2: 同上"),
    "AnyMap::every": ("broken", "A4: 返回 &Self 而不是 bool，不能作谓词"),
    "AnyMap::push_datum": ("limited", "B3: 不去重，重复 key 会出现多次"),
    "Dir::join": ("broken", "A5: PathBuf::join 返回值被丢弃，是空操作"),
    "Dir::up": ("limited", "B12: 相对路径下失效，会原样返回自身"),
    "Dir::new": ("limited", "B11: 递归扫描整棵子树，大目录开销巨大"),
    "Dir::zip": ("limited", "B15/C1: 产物不是标准 ZIP；unzip 不校验路径逃逸"),
    "Dir::unzip": ("limited", "C1: 不校验路径逃逸，不要用于不可信归档"),
    "File::mv": ("limited", "B8: 失败写 err 字段，成功后不清除旧 err"),
    "File::read": ("limited", "B9: 依赖构造时的 exist 快照"),
    "File::write": ("limited", "B10: 覆盖写入，不是追加"),
    "HttpResponse::deleted": ("limited", "B7: code 204 在 HTTP 标准里禁止响应体，这里仍可 content()"),
    "HttpResponse::updated": ("limited", "B6: 参数是 Option<String>，其余构造器是 Option<&str>"),
    "Lz4::compress_with_level": ("limited", "A7: level 参数不影响输出"),
    "with_lz4_level": ("limited", "A7: level 参数不影响输出"),
    "Aes::cbc": ("limited", "C6: 无完整性校验，存在 padding oracle 风险"),
    "Aes::ctr": ("limited", "C6: 无完整性校验，任意输入都不会解密失败"),
    "Aes::ecb": ("limited", "C6: 泄露数据模式，无完整性校验"),
    "Aes::gcm": ("limited", "C5/C7: nonce 不可复用；key/iv 在 Drop 时不清零"),
    "impl_singleton!": ("broken", "A6: 展开引用 pub(crate) Singleton，在外部 crate 整体编译失败"),
    # ORM：这些不是 bug，是使用时必须知道的边界。
    "QueryBuilder::r#where": ("limited", "ORM1: 列名必须在 E::COLUMNS 白名单内，非法列名延后到 all()/one()/count() 报 InvalidInput"),
    "QueryBuilder::order_by": ("limited", "ORM2: 列名必须在 E::COLUMNS 白名单内"),
    "Repository::transaction": ("limited", "ORM3: 只支持单层事务，嵌套返回 InvalidInput；Err 路径自动 rollback"),
    "Repository::get": ("limited", "ORM4: 未找到返回 NotFound；find_one 则返 Ok(None)"),
    "SqliteDriver::execute_batch": ("limited", "ORM5: 直接执行原始 SQL，仅供 DDL/测试，不经过参数化"),
    # 编号说明：生成器里的 ORM1-ORM5 与 spec/30 的 ORM1-ORM7 是两套历史编号，
    # 语义已错位，本次不重排；MySQL 驱动相关标注统一用 MYSQL* 前缀。
    "MysqlDriver::execute_ddl": ("limited", "MYSQL5: 单条原始 DDL，不参数化；实测客户端允许多语句，绝不接用户输入"),
    "MysqlDriver::connect_with_tz": ("limited", "MYSQL7: 双时区自动转换仅对 Value::DateTime 生效；TIMESTAMP/DATETIME 读回一律 Value::DateTime；固定偏移无 DST，SQLite 不支持"),
}

# Declared but never implemented; using them is a compile/behavior trap.
UNIMPLEMENTED_TRAITS = {"AnySliceTrait", "AnyMapTrait"}


def manifest() -> dict:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")

    def scalar(key: str) -> str | None:
        m = re.search(rf'^\s*{re.escape(key)}\s*=\s*"([^"]+)"', text, re.M)
        return m.group(1) if m else None

    m = re.search(r"crate-type\s*=\s*\[([^\]]+)\]", text)
    crate_types = re.findall(r'"([^"]+)"', m.group(1)) if m else []
    return {
        "name": scalar("name"),
        "version": scalar("version"),
        "edition": scalar("edition"),
        "description": scalar("description"),
        "license": scalar("license"),
        "repository": scalar("repository"),
        "homepage": scalar("homepage"),
        "crate_types": crate_types,
        "msrv": MSRV,
        "msrv_source": "aes 0.9 声明 rust-version 1.89，高于 edition 2024 的 1.85",
    }


def source_root_of(path: Path) -> tuple[Path, str]:
    """返回文件所属的 (源根, crate 前缀)。"""
    resolved = path.resolve()
    for root, prefix in SOURCE_ROOTS:
        try:
            resolved.relative_to(root.resolve())
            return root, prefix
        except ValueError:
            continue
    raise ValueError(f"文件不属于任何源根: {path}")


def module_path_of(path: Path) -> str:
    root, prefix = source_root_of(path)
    parts = list(path.resolve().relative_to(root.resolve()).parts)
    if parts[-1] == "mod.rs":
        parts = parts[:-1]
    else:
        parts[-1] = parts[-1][:-3]
    if parts == ["lib"]:
        return prefix
    return prefix + "::" + "::".join(parts)


def all_rust_files() -> list[Path]:
    files: list[Path] = []
    for root, _ in SOURCE_ROOTS:
        files.append(root / "lib.rs")
        files.extend(sorted(root.rglob("*.rs")))
    # 去重且只保留存在的普通文件
    seen: set[Path] = set()
    result = []
    for path in files:
        resolved = path.resolve()
        if resolved in seen or not path.is_file():
            continue
        seen.add(resolved)
        result.append(path)
    return result


def collect_modules() -> list[dict]:
    decls: list[dict] = []
    for path in [root / "lib.rs" for root, _ in SOURCE_ROOTS] + [
        p for root, _ in SOURCE_ROOTS for p in sorted(root.rglob("mod.rs"))
    ]:
        if not path.is_file():
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        parent = module_path_of(path)
        for i, line in enumerate(lines):
            m = re.match(r"\s*(pub\s+)?mod\s+([A-Za-z_]\w*)\s*;", line)
            if not m:
                continue
            visibility = "pub" if m.group(1) else "private"
            name = m.group(2)
            cfg = None
            if i > 0:
                cm = re.search(r"#\[cfg\((.*?)\)\]", lines[i - 1])
                if cm:
                    cfg = cm.group(1)
            base = path.parent / name
            file = None
            if (base / "mod.rs").exists():
                file = base / "mod.rs"
            elif base.with_suffix(".rs").exists():
                file = base.with_suffix(".rs")
            decls.append({
                "path": f"{parent}::{name}",
                "visibility": visibility,
                "cfg": cfg,
                "test_only": cfg == "test",
                "declaration": line.strip(),
                "declared_in": str(path.relative_to(ROOT)),
                "file": str(file.relative_to(ROOT)) if file else None,
            })
    return decls


def normalize(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def strip_literals(text: str) -> str:
    """去掉字符串/字符/注释内容，避免其中的花括号干扰深度计算。

    逐字符状态机：普通字符串、原始字符串、字节字符串、字符字面量、行注释。
    保留括号本身与所有结构字符。
    """
    out: list[str] = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        # 行注释
        if ch == "/" and i + 1 < n and text[i + 1] == "/":
            break
        # 原始字符串 r"..."/r#"..."#
        if ch == "r" and i + 1 < n and (text[i + 1] == '"' or text[i + 1] == '#'):
            j = i + 1
            hashes = 0
            while j < n and text[j] == "#":
                hashes += 1
                j += 1
            if j < n and text[j] == '"':
                terminator = '"' + "#" * hashes
                end = text.find(terminator, j + 1)
                i = n if end == -1 else end + len(terminator)
                out.append(" ")
                continue
        # 普通字符串 / 字节字符串
        if ch == '"' or (ch == "b" and i + 1 < n and text[i + 1] == '"'):
            j = i + (2 if ch == "b" else 1)
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            i = j
            out.append(" ")
            continue
        # 字符字面量仅在明显闭合时跳过（生命周期 'a 没有闭合引号，会落到 else）
        if ch == "'":
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == "'":
                    j += 1
                    break
                j += 1
            if j < n and text[j - 1] == "'":
                i = j
                out.append(" ")
                continue
        out.append(ch)
        i += 1
    return "".join(out)


def brace_delta(line: str) -> int:
    stripped = strip_literals(line)
    return stripped.count("{") - stripped.count("}")


def skip_block(lines: list[str], start: int) -> int:
    """Return the index after a balanced `{ ... }` block opened at `start`."""
    depth = 0
    i = start
    while i < len(lines):
        depth += brace_delta(lines[i])
        i += 1
        if depth == 0 and i > start:
            break
    return i


def split_top_level(text: str, sep: str = ",") -> list[str]:
    out, buf, level = [], [], 0
    for ch in text:
        if ch in "([{<":
            level += 1
        elif ch in ")]}>":
            level -= 1
        if ch == sep and level == 0:
            out.append("".join(buf).strip())
            buf = []
        else:
            buf.append(ch)
    if "".join(buf).strip():
        out.append("".join(buf).strip())
    return out


def abi_type(rust_type: str) -> str:
    t = rust_type.strip()
    if t.startswith("*"):
        return "i32 (ptr)"
    if t in {"usize", "u32", "i32"}:
        return "i32"
    return t


def parse_export(signature: str, name: str, file: str, line: int, is_unsafe: bool) -> dict:
    start = signature.find("(")
    level, end = 0, start
    for j in range(start, len(signature)):
        if signature[j] == "(":
            level += 1
        elif signature[j] == ")":
            level -= 1
            if level == 0:
                end = j
                break
    inner = signature[start + 1:end]
    rest = signature[end + 1:].strip()
    rm = re.match(r"->\s*(.+?)\s*(?:where\s|$)", rest)
    returns = normalize(rm.group(1)) if rm else "()"
    params = []
    for raw in split_top_level(inner):
        if ":" in raw:
            pname, ptype = raw.split(":", 1)
            params.append({
                "name": pname.strip(),
                "rust_type": normalize(ptype),
                "abi_type": abi_type(ptype),
            })
    return {
        "name": name,
        "signature": signature,
        "params": params,
        "returns": returns,
        "returns_abi": abi_type(returns),
        "unsafe": is_unsafe,
        "file": file,
        "line": line,
    }


def parse_impl(header: str) -> tuple[str | None, str]:
    """Extract the owner type name and impl kind from an `impl ... {` header."""
    text = normalize(header)
    text = re.sub(r"^(?:unsafe\s+)?impl\s*", "", text)
    text = re.sub(r"^<[^>]*>\s*", "", text)  # drop generic parameters
    is_trait_impl = bool(re.search(r"\bfor\b", text))
    if is_trait_impl:
        text = text.split(" for ", 1)[1]
    text = text.split("{", 1)[0].strip()
    m = re.match(r"([A-Za-z_]\w*)", text)
    return (m.group(1) if m else None), ("trait_impl" if is_trait_impl else "inherent_impl")


def annotation_for(owner: str | None, name: str) -> tuple[str, str | None]:
    if owner in UNIMPLEMENTED_TRAITS or name in UNIMPLEMENTED_TRAITS:
        return ("unimplemented", "B23: trait 只有声明没有实现，不要 use")
    key = f"{owner}::{name}" if owner else name
    ann = ANNOTATIONS.get(key) or ANNOTATIONS.get(name)
    return (ann[0], ann[1]) if ann else ("ok", None)


def parse_file(path: Path) -> tuple[list[dict], list[dict]]:
    rel = str(path.relative_to(ROOT))
    if path.name.endswith("_test.rs") or path.name == "test.rs":
        return [], []  # test modules are not public API

    lines = path.read_text(encoding="utf-8").splitlines()
    module = module_path_of(path)
    items: list[dict] = []
    exports: list[dict] = []
    owner_stack: list[tuple[str, str, int]] = []  # (name, kind, depth when opened)
    pending_attrs: list[str] = []
    depth = 0
    i = 0
    impl_acc: list[str] | None = None

    while i < len(lines):
        raw = lines[i]
        line = raw.strip()
        i += 1

        # Depth accounting must run for every line, including the branches
        # that `continue` below; otherwise impl/struct scopes drift.
        depth += brace_delta(raw)
        while owner_stack and depth < owner_stack[-1][2]:
            owner_stack.pop()

        if not line:
            continue
        if line.startswith("///") or line.startswith("//!"):
            continue
        if line.startswith("#["):
            pending_attrs.append(line)
            # skip an entire #[cfg(test)] mod test { ... } block
            if "cfg(test)" in line:
                j = i
                while j < len(lines) and not lines[j].strip():
                    j += 1
                if j < len(lines) and re.match(r"\s*(pub\s+)?mod\s+\w+\s*\{", lines[j]):
                    end = skip_block(lines, j)
                    depth += sum(brace_delta(l) for l in lines[j:end])
                    i = end
                    pending_attrs = []
            continue

        attrs = pending_attrs
        pending_attrs = []
        cfg = None
        for a in attrs:
            cm = re.search(r"#\[cfg\((.*?)\)\]", a)
            if cm:
                cfg = cm.group(1)
        no_mangle = any("no_mangle" in a for a in attrs)

        # impl blocks give methods their owner name (used by ANNOTATIONS).
        if impl_acc is not None:
            impl_acc.append(line)
            if "{" in line:
                owner, impl_kind = parse_impl(" ".join(impl_acc))
                if owner:
                    owner_stack.append((owner, impl_kind, depth))
                impl_acc = None
            continue

        if re.match(r"(unsafe\s+)?impl\b", line):
            if "{" in line:
                owner, impl_kind = parse_impl(line)
                if owner:
                    owner_stack.append((owner, impl_kind, depth))
            else:
                impl_acc = [line]
            continue

        # macro_rules! bodies contain generated `pub fn` text; skip the body.
        mm = re.match(r"macro_rules!\s+([A-Za-z_]\w*)", line)
        if mm:
            name = mm.group(1) + "!"
            status, note = annotation_for(None, name)
            items.append({
                "path": f"{module}::{name}",
                "module": module,
                "kind": "macro",
                "name": name,
                "owner": None,
                "signature": line,
                "cfg": cfg,
                "attributes": attrs or None,
                "status": status,
                "note": note,
                "file": rel,
                "line": i,
            })
            if "{" in line:
                first = i - 1
                end = skip_block(lines, first)
                depth += sum(brace_delta(l) for l in lines[first + 1:end])
                i = end
            continue

        # fn items: `pub fn` plus trait / trait-impl methods (no `pub` prefix).
        fm = re.match(
            r"(pub\s+)?(async\s+)?(unsafe\s+)?(const\s+)?(extern\s+\"C\"\s+)?fn\s+(r#)?([A-Za-z_]\w*)",
            line,
        )
        if fm:
            is_pub = bool(fm.group(1))
            is_unsafe = bool(fm.group(3))
            is_extern = bool(fm.group(5))
            raw_prefix = fm.group(6) or ""
            name = raw_prefix + fm.group(7)
            owner_kind = owner_stack[-1][1] if owner_stack else None
            # private free functions and inherent private methods are not public API
            is_public = is_pub or owner_kind in {"trait", "trait_impl"}
            buf = [line]
            if "{" not in line and not line.rstrip().endswith(";"):
                while i < len(lines):
                    nxt = lines[i].strip()
                    buf.append(nxt)
                    i += 1
                    if "{" in nxt or nxt.rstrip().endswith(";"):
                        break
            for extra in buf[1:]:
                depth += brace_delta(extra)
            while owner_stack and depth < owner_stack[-1][2]:
                owner_stack.pop()
            if not is_public:
                continue
            sig = normalize(" ".join(buf))
            # cut at the function body / declaration terminator
            cut = min([p for p in (sig.find("{"), len(sig) if sig.endswith(";") else len(sig)) if p >= 0])
            sig = sig[:cut].rstrip().rstrip(";").rstrip()
            sig = re.sub(r",\s*$", "", sig)  # trailing comma in a multiline where clause
            sig = sig.replace("( ", "(").replace(" )", ")").replace(" ,", ",")
            sig = re.sub(r",\s*\)", ")", sig)
            owner = owner_stack[-1][0] if owner_stack else None
            status, note = annotation_for(owner, name)
            platform = "all"
            item = {
                "path": f"{module}::{owner}::{name}" if owner else f"{module}::{name}",
                "module": module,
                "kind": "fn",
                "name": name,
                "owner": owner,
                "visibility": "pub" if is_pub else "inherited (trait method)",
                "signature": sig,
                "cfg": cfg,
                "attributes": attrs or None,
                "status": status,
                "note": note,
                "platform": platform,
                "file": rel,
                "line": i - len(buf) + 1,
            }
            if no_mangle:
                item["export"] = name
                exports.append(parse_export(sig, name, rel, item["line"], is_unsafe))
            if is_extern:
                item["extern"] = "C"
            items.append(item)
            continue

        # public struct / enum / trait / type / const / static / use
        dm = re.match(r"pub\s+(unsafe\s+)?(struct|enum|trait|type|const|static|use)\s+([A-Za-z_]\w*)", line)
        if dm:
            kind, name = dm.group(2), dm.group(3)
            status, note = annotation_for(None, name)
            decl = re.sub(r"\s*\{\s*$", "", normalize(line))
            items.append({
                "path": f"{module}::{name}",
                "module": module,
                "kind": kind,
                "name": name,
                "owner": None,
                "visibility": "pub",
                "signature": decl,
                "cfg": cfg,
                "attributes": attrs or None,
                "status": status,
                "note": note,
                "file": rel,
                "line": i,
            })
            if kind in {"struct", "enum", "trait"} and brace_delta(line) > 0:
                owner_stack.append((name, kind, depth))
            continue

        # public struct fields
        if owner_stack and owner_stack[-1][1] == "struct":
            fld = re.match(r"pub\s+([A-Za-z_]\w*)\s*:\s*(.+?),?\s*$", line)
            if fld:
                owner = owner_stack[-1][0]
                items.append({
                    "path": f"{module}::{owner}::{fld.group(1)}",
                    "module": module,
                    "kind": "field",
                    "name": fld.group(1),
                    "owner": owner,
                    "visibility": "pub",
                    "signature": re.sub(r",\s*$", "", normalize(line)),
                    "cfg": cfg,
                    "attributes": attrs or None,
                    "status": "ok",
                    "note": None,
                    "file": rel,
                    "line": i,
                })

    return items, exports


def build() -> dict:
    modules = collect_modules()
    items: list[dict] = []
    exports: list[dict] = []
    for path in all_rust_files():
        file_items, file_exports = parse_file(path)
        items.extend(file_items)
        exports.extend(file_exports)

    by_kind: dict[str, int] = {}
    by_status: dict[str, int] = {}
    for it in items:
        by_kind[it["kind"]] = by_kind.get(it["kind"], 0) + 1
        by_status[it["status"]] = by_status.get(it["status"], 0) + 1

    return {
        "spec": SPEC_VERSION,
        "generated_by": "docs/ai/tools/gen_api_index.py",
        "generator_version": GENERATOR_VERSION,
        "source_roots": [str(root.relative_to(ROOT)) for root, _ in SOURCE_ROOTS],
        "crates": [
            {
                "name": name,
                "manifest": str(path.relative_to(ROOT)),
                "version": re.search(r'^version\s*=\s*"([^"]+)"', path.read_text(encoding="utf-8"), re.M).group(1),
            }
            for name, path in MANIFESTS
            if path.is_file()
        ],
        "crate": manifest(),  # 兼容旧消费者：主 crate 的 manifest 摘要
        "platforms": ["native"],
        "status_legend": {
            "ok": "可用，无特别限制",
            "broken": "实现失效，禁止依赖；见 spec/30-known-issues.md",
            "limited": "可用但有已知陷阱或安全边界；见 note 与 spec/30-known-issues.md",
            "unimplemented": "trait 只有声明没有实现，禁止 use；见 note",
        },
        "issue_index": "../spec/30-known-issues.md",
        "modules": modules,
        "items": items,
        "exports": sorted(exports, key=lambda e: e["name"]),
        "counts": {
            "modules": len(modules),
            "items": len(items),
            "by_kind": dict(sorted(by_kind.items())),
            "by_status": dict(sorted(by_status.items())),
            "exports": len(exports),
        },
    }


def main() -> int:
    data = build()
    text = json.dumps(data, ensure_ascii=False, indent=2) + "\n"
    if "--check" in sys.argv:
        if not OUT.exists():
            print(f"MISSING: {OUT.relative_to(ROOT)} — run the generator without --check", file=sys.stderr)
            return 1
        current = OUT.read_text(encoding="utf-8")
        if current != text:
            print("STALE: public-api.json differs from the sources. Regenerate with:", file=sys.stderr)
            print("  python3 docs/ai/tools/gen_api_index.py", file=sys.stderr)
            old = json.loads(current)
            for key in ("crate", "counts"):
                if old.get(key) != data.get(key):
                    print(f"  {key}: {old.get(key)} -> {data[key]}", file=sys.stderr)
            return 1
        print(f"OK: {OUT.relative_to(ROOT)} is in sync ({data['counts']['items']} items, {data['counts']['exports']} exports)")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}: {data['counts']['items']} items, {data['counts']['exports']} exports")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
