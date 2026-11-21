#!/usr/bin/env python3
"""Check a translation file of the NPT mobile wallet against en.ts.

    python3 scripts/check-translations.py src/i18n/en.ts src/i18n/<code>.ts

Checks: same keys in the same order, no duplicates, identical {placeholders},
the same number of **bold** markers, untranslated product terms kept, and no
value left in English (identical to en.ts) unless it is allowed to be.
"""
import re
import sys

PAIR = re.compile(r'"([A-Za-z0-9_.]+)"\s*:\s*"((?:[^"\\]|\\.)*)"', re.S)
PLACEHOLDER = re.compile(r"\{(\w+)\}")
# Terms that must appear unchanged when the English text contains them.
KEEP = ["NPT", "UTXO", "Generation", "EC-hybrid", "Viewing", "--utxo-index", "https://", "nechm1", "nolgam1", "nviewm1", "#"]
# Values that may legitimately stay identical to English.
SAME_OK = {
    "common.ok", "send.max", "settings.minutes", "import.wordCount", "wallet.synced", "settings.describe",
    "tx.changeBack", "bubble.proving", "settings.works", "settings.connected", "wallet.receiveTitle",
    "addresses.title", "send.addressType", "history.receivedCount", "history.sentCount", "unlock.lockedFor",
    "overlay.to", "history.to", "history.fee", "settings.hidesIn", "errors.invalidAddressShort",
    "time.minutesAgo", "time.hoursAgo", "wallet.locked", "send.spendable", "tx.lockedUntil", "send.recipientPlaceholder",
    "tx.status", "settings.node", "send.total", "tx.date", "nav.wallet", "tx.block",
    "timeline.transaction", "time.shortMinutes", "time.shortHours", "time.shortDays",
}


def load(path):
    text = open(path, encoding="utf-8").read()
    body = text[text.index("{") :]
    pairs = PAIR.findall(body)
    return pairs


def main():
    en_path, tr_path = sys.argv[1], sys.argv[2]
    en = load(en_path)
    tr = load(tr_path)
    problems = []
    en_keys = [k for k, _ in en]
    tr_keys = [k for k, _ in tr]
    dupes = sorted({k for k in tr_keys if tr_keys.count(k) > 1})
    if dupes:
        problems.append(f"duplicate keys: {dupes}")
    missing = [k for k in en_keys if k not in tr_keys]
    extra = [k for k in tr_keys if k not in en_keys]
    if missing:
        problems.append(f"missing keys ({len(missing)}): {missing[:20]}")
    if extra:
        problems.append(f"unknown keys ({len(extra)}): {extra[:20]}")
    if not missing and not extra and en_keys != tr_keys:
        problems.append("keys are not in the same order as en.ts")
    en_map, tr_map = dict(en), dict(tr)
    identical = []
    for key in en_keys:
        if key not in tr_map:
            continue
        src, dst = en_map[key], tr_map[key]
        if sorted(PLACEHOLDER.findall(src)) != sorted(PLACEHOLDER.findall(dst)):
            problems.append(f"{key}: placeholders {PLACEHOLDER.findall(src)} became {PLACEHOLDER.findall(dst)}")
        if src.count("**") != dst.count("**"):
            problems.append(f"{key}: bold markers ** count {src.count('**')} became {dst.count('**')}")
        for term in KEEP:
            if term in src and term not in dst:
                problems.append(f"{key}: keep the term {term!r} untranslated")
        if not dst.strip():
            problems.append(f"{key}: empty value")
        if dst == src and key not in SAME_OK and len(src) > 3:
            identical.append(key)
    if identical:
        problems.append(f"still identical to English ({len(identical)}): {identical[:25]}")
    print(f"{tr_path}: {len(tr_keys)} keys (en.ts has {len(en_keys)})")
    if problems:
        print("PROBLEMS:")
        for p in problems:
            print(" -", p)
        sys.exit(1)
    print("OK")


if __name__ == "__main__":
    main()
