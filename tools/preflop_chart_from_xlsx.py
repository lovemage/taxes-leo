#!/usr/bin/env python3
"""把顧問的 9MAX 翻前組合表（.xlsx）轉成引擎的內容資產。

引擎沒有任何相依（連 serde 都沒有），也不會在執行期讀 Excel：內容一律
離線轉成純文字資產，隨二進位檔一起編譯進去。這支腳本是那個轉換。

用法：
    python3 tools/preflop_chart_from_xlsx.py \
        docs/9MAX手牌組合_6.xlsx \
        apps/engine/assets/preflop-default-chart-v1.txt

轉換過程會驗證來源表本身的一致性，任何一項不成立就中止而不產檔：
  * 每一格的手牌代號都是合法的 169 類之一；
  * 同一（深度 × 位置 × 情境）下五個動作的手牌互不重疊；
  * 「其餘手牌」補完後合計恰為 1,326 個 combo；
  * 表列百分比與實際 combo 數相符（容差為四捨五入的 0.001）。
"""

import sys
from pathlib import Path

import openpyxl

ASSET_FORMAT = 1

# 來源表的欄位值 → 資產的欄位鍵。
DEPTHS = {"0-15BB": "0-15", "35-50BB": "35-50", "100BB": "100", "200-250BB": "200-250"}
# 來源表把 BTN 寫成 BNT；位置標籤以規則細則 8.4.1 的字串為準。
POSITIONS = {
    "UTG": "UTG",
    "UTG+1": "UTG+1",
    "UTG+2": "UTG+2",
    "LJ": "LJ",
    "HJ": "HJ",
    "CO": "CO",
    "BNT": "BTN",
    "SB": "SB",
    "BB": "BB",
}
SCENARIOS = {
    "前面無人加注": "unopened",
    "OPEN": "open",
    "OPEN-RAICE": "open-raise",
    "3B": "3bet",
    "4B": "4bet",
}
ACTIONS = {
    "蓋牌": "fold",
    "跟注": "call",
    "加注(前方2.5倍)": "raise-2.5x",
    "加注(前方8倍)": "raise-8x",
    "ALL IN": "allin",
}

DEPTH_ORDER = list(DEPTHS.values())
POSITION_ORDER = list(POSITIONS.values())
SCENARIO_ORDER = list(SCENARIOS.values())
ACTION_ORDER = list(ACTIONS.values())

# 選擇子：資產的第五欄。
NONE, REST, ALL = "-", "*", "+"

RANK_VALUE = {ch: v for ch, v in zip("23456789TJQKA", range(2, 15))}
TOTAL_COMBOS = 1326


def class_index(label):
    """複製 HandClass::index()：13×13 網格，列與欄皆由 A 到 2。"""
    if len(label) == 2:
        high, low, suited = label[0], label[1], False
    else:
        high, low, suited = label[0], label[1], label[2] == "s"
    hi = 14 - RANK_VALUE[high]
    lo = 14 - RANK_VALUE[low]
    row, col = (hi, lo) if suited else (lo, hi)
    return row * 13 + col


def combos(label):
    if len(label) == 2:
        return 6
    return 4 if label[2] == "s" else 12


def valid_label(label):
    if len(label) == 2:
        return label[0] == label[1] and label[0] in RANK_VALUE
    if len(label) != 3 or label[2] not in "so":
        return False
    high, low = label[0], label[1]
    return high in RANK_VALUE and low in RANK_VALUE and RANK_VALUE[high] > RANK_VALUE[low]


def read_rows(path):
    """讀出來源表並把合併欄位向下填滿。"""
    sheet = openpyxl.load_workbook(path, data_only=True)["Sheet1"]
    carried = [None] * 5
    out = []
    for row in sheet.iter_rows(min_row=2, values_only=True):
        row = list(row)
        for i in range(5):
            if row[i] not in (None, ""):
                carried[i] = row[i]
            row[i] = carried[i]
        out.append(row)
    return out


def parse(rows):
    """→ {(depth, position, scenario): {action: (selector, labels, note)}}"""
    groups = {}
    for table, depth, position, scenario, action, hands, percent, note in rows:
        if table != "9MAX":
            raise SystemExit(f"來源表只應含 9MAX，卻出現「{table}」")
        key = (DEPTHS[depth], POSITIONS[position], SCENARIOS[scenario])
        cell = groups.setdefault(key, {})
        action_key = ACTIONS[action]
        if action_key in cell:
            raise SystemExit(f"{key} 的「{action}」出現兩次")

        text = "" if hands is None else str(hands).strip()
        if text == "無":
            selector, labels = NONE, []
        elif text == "其餘手牌":
            selector, labels = REST, []
        elif text.startswith("全部手牌"):
            # BB 在無人加注時已投入大盲，這一列的語意是過牌而不是跟注。
            selector, labels = ALL, []
        else:
            selector = ""
            labels = [h.strip() for h in text.replace("，", ",").split(",") if h.strip()]
            for label in labels:
                if not valid_label(label):
                    raise SystemExit(f"{key} 的「{action}」含非法牌類：{label}")
        note = "" if note is None else " ".join(str(note).split())
        if "|" in note:
            raise SystemExit(f"{key} 的「{action}」說明含分隔字元 |")
        cell[action_key] = (selector, labels, note, percent or 0.0)
    return groups


def validate(groups):
    expected = len(DEPTH_ORDER) * len(POSITION_ORDER) * len(SCENARIO_ORDER)
    if len(groups) != expected:
        raise SystemExit(f"應有 {expected} 組，實得 {len(groups)}")

    for key, cell in groups.items():
        if set(cell) != set(ACTION_ORDER):
            raise SystemExit(f"{key} 的動作不齊：{sorted(cell)}")

        seen = {}
        used = 0
        for action in ACTION_ORDER:
            selector, labels, _, percent = cell[action]
            for label in labels:
                if label in seen:
                    raise SystemExit(f"{key} 的 {label} 同時出現在 {seen[label]} 與 {action}")
                seen[label] = action
                used += combos(label)
            if selector == "":
                got = sum(combos(l) for l in labels) / TOTAL_COMBOS
                if abs(got - percent) > 0.001:
                    raise SystemExit(f"{key}/{action} 百分比 {percent} 與實際 {got:.4f} 不符")

        rest = TOTAL_COMBOS - used
        fold_selector, _, _, fold_percent = cell["fold"]
        if fold_selector == REST:
            if rest <= 0:
                raise SystemExit(f"{key} 的「其餘手牌」補完後沒有牌可棄")
            if abs(rest / TOTAL_COMBOS - fold_percent) > 0.001:
                raise SystemExit(
                    f"{key}/fold 百分比 {fold_percent} 與其餘 {rest / TOTAL_COMBOS:.4f} 不符"
                )
        elif any(cell[a][0] == ALL for a in ACTION_ORDER):
            if used != 0:
                raise SystemExit(f"{key} 同時有「全部手牌」與逐手清單")
        elif used != 0:
            raise SystemExit(f"{key} 沒有「其餘手牌」卻列了 {used} 個 combo")


FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x00000100000001B3
MASK = (1 << 64) - 1


def feed_bytes(hash_value, data):
    for byte in data:
        hash_value ^= byte
        hash_value = (hash_value * FNV_PRIME) & MASK
    return hash_value


def feed(hash_value, value):
    return feed_bytes(hash_value, value.to_bytes(8, "little"))


def checksum(groups):
    """FNV-1a 走過解析後的內容，因此註解、空白與換行都不影響結果。"""
    hash_value = feed(FNV_OFFSET, ASSET_FORMAT)
    for depth in DEPTH_ORDER:
        for position in POSITION_ORDER:
            for scenario in SCENARIO_ORDER:
                cell = groups[(depth, position, scenario)]
                for index, action in enumerate(ACTION_ORDER):
                    selector, labels, note, _ = cell[action]
                    hash_value = feed(hash_value, DEPTH_ORDER.index(depth))
                    hash_value = feed(hash_value, POSITION_ORDER.index(position))
                    hash_value = feed(hash_value, SCENARIO_ORDER.index(scenario))
                    hash_value = feed(hash_value, index)
                    hash_value = feed(hash_value, {NONE: 0, REST: 1, ALL: 2, "": 3}[selector])
                    for label in sorted(class_index(l) for l in labels):
                        hash_value = feed(hash_value, label)
                    hash_value = feed_bytes(hash_value, note.encode("utf-8"))
    return hash_value


HEADER = """\
# 9max 模擬平台 — 翻前預設組合表
#
# 這是離線產製的內容，請勿手改。重新產生：
#   python3 tools/preflop_chart_from_xlsx.py docs/9MAX手牌組合_6.xlsx
#
# 來源是 9MAX 滿桌的顧問表。6–8 人桌不另立內容，改由刪位置得到
# （8 人刪 UTG+2；7 人刪 UTG+1、UTG+2；6 人刪 UTG+1、UTG+2、LJ），
# 與規則細則 8.4.1 的位置序列一致。
#
# 每一列是一格：深度|位置|情境|動作|手牌|說明
#   手牌  `-` 無　`*` 其餘手牌（同組其他四個動作沒收走的）
#         `+` 全部手牌　其餘為逗號分隔的 169 類代號
#   說明  來源表 H 欄的原文，面板逐格顯示
#
# 校驗碼算在解析後的內容上，不是原始位元組：註解與換行因此都不參與，
# Windows 端 checkout 出 CRLF 也不會讓資產突然載不進來。
"""


def encode(groups):
    lines = [HEADER, f"format {ASSET_FORMAT}", "source 9MAX手牌組合_6.xlsx",
             f"checksum {checksum(groups):#018x}", ""]
    for depth in DEPTH_ORDER:
        for position in POSITION_ORDER:
            lines.append(f"# {depth}BB · {position}")
            for scenario in SCENARIO_ORDER:
                cell = groups[(depth, position, scenario)]
                for action in ACTION_ORDER:
                    selector, labels, note, _ = cell[action]
                    hands = selector if selector else ", ".join(labels)
                    lines.append(f"{depth}|{position}|{scenario}|{action}|{hands}|{note}")
            lines.append("")
    return "\n".join(lines)


def main():
    source = Path(sys.argv[1] if len(sys.argv) > 1 else "docs/9MAX手牌組合_6.xlsx")
    target = Path(
        sys.argv[2] if len(sys.argv) > 2 else "apps/engine/assets/preflop-default-chart-v1.txt"
    )
    groups = parse(read_rows(source))
    validate(groups)
    target.write_text(encode(groups), encoding="utf-8")
    print(f"{target}：{len(groups)} 組 × {len(ACTION_ORDER)} 動作，校驗碼 {checksum(groups):#018x}")


if __name__ == "__main__":
    main()
