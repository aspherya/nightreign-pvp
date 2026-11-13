"""Add saved presets to the existing Sparring Equipment interaction, retaining its prompt and interruption guards.

Input is esdtool's decompilation of the pinned NR 1.03.3 t607001000. Only our added code is generated here;
no game script is distributed. Build with scripts/build-loadouts.ps1, which checks the original bundle hash.
Protocol: save 10007800..809, load 810..819, occupied 820..829, ready/done/error 830..832 and refresh 834 (833 and the old hero flags are reserved). All requests are local.
"""
import re
import sys
from pathlib import Path

SCRIPT = "t607001000"
SAVE, LOAD, OCCUPIED = 10007800, 10007810, 10007820
READY, DONE, ERROR, REFRESH = 10007830, 10007831, 10007832, 10007834
TEXT = 99101000
WAIT_MENU = "assert not (CheckSpecificPersonMenuIsOpen(12, 0) and not CheckSpecificPersonGenericDialogIsOpen(0))"


def menu(lines):
    return ["ClearPreviousMenuSelection()", "ClearTalkListData()", *lines,
            "OpenConversationChoicesMenu(0)", WAIT_MENU]


def machine(number, lines):
    return f"\ndef {SCRIPT}_x{number}():\n" + "\n".join("    " + line if line else "" for line in lines) + "\n"


def patch(source):
    if source.count(f"def {SCRIPT}_x3():") != 1 or "OpenSparringEquipmentMenu()" not in source:
        raise ValueError("Expected the original Sparring Equipment state machine")
    if re.search(rf"def {SCRIPT}_x9\d+\(", source):
        raise ValueError("Reserved preset machines already present; refusing to patch twice")
    # Existing outer x2 still calls x3 and retains its distance/death guards. x903 holds the stock equipment opener.
    source = source.replace(f"def {SCRIPT}_x3():", f"def {SCRIPT}_x903():")
    lines = ["while True:"]
    top = menu([
        f"AddTalkListDataAlt(1, {TEXT}, -1, 0, False)",
        f"AddTalkListDataAltIf(GetEventFlag({READY}), 2, {TEXT + 1}, -1, 0, False)",
        f"AddTalkListDataAltIf(GetEventFlag({READY}), 3, {TEXT + 2}, -1, 0, False)",
        f"AddTalkListDataAlt(4, {TEXT + 3}, -1, 0, False)",
    ]) + ["if GetTalkListEntryResult() == 1:", f"    assert {SCRIPT}_x903()",
          "elif GetTalkListEntryResult() == 2:", f"    assert {SCRIPT}_x901()",
          "elif GetTalkListEntryResult() == 3:", f"    assert {SCRIPT}_x902()", "else:", "    return 0"]
    lines += ["    " + line for line in top]
    source += machine(3, lines)
    for save, number in [(True, 901), (False, 902)]:
        lines = [f"SetEventFlag({REFRESH}, FlagState.On)",
                 f"assert not GetEventFlag({REFRESH}) or GetCurrentStateElapsedTime() > 3",
                 f"if GetEventFlag({REFRESH}):", "    return 0", "else:", "    pass"]
        choices = []
        for slot in range(10):
            choices += [f"AddTalkListDataAltIf(not GetEventFlag({OCCUPIED + slot}), {slot + 1}, {TEXT + 20 + slot}, -1, 0, False)"]
            choices += [f"AddTalkListDataAltIf(GetEventFlag({OCCUPIED + slot}), {slot + 1}, {TEXT + 30 + slot}, -1, 0, False)"]
        choices += [f"AddTalkListDataAlt(11, {TEXT + 3}, -1, 0, False)"]
        lines += menu(choices)
        for slot in range(10):
            lines += [f"{'if' if slot == 0 else 'elif'} GetTalkListEntryResult() == {slot + 1}:"]
            branch = []
            if save:
                branch += [f"if GetEventFlag({OCCUPIED + slot}):"]
                branch += ["    " + line for line in menu([
                    f"AddTalkListDataAlt(1, {TEXT + 4}, -1, 0, False)",
                    f"AddTalkListDataAlt(2, {TEXT + 3}, -1, 0, False)",
                ])]
                branch += ["    if GetTalkListEntryResult() != 1:", "        return 0", "    else:", "        pass", "else:", "    pass"]
            else:
                branch += [f"if not GetEventFlag({OCCUPIED + slot}):", f"    assert {SCRIPT}_x912()", "    return 0", "else:", "    pass"]
            # Slots have their own no-argument machine, so esdtool never needs to infer arithmetic on machine arguments.
            branch += [f"assert {SCRIPT}_x{920 + slot if save else 940 + slot}()"]
            lines += ["    " + line for line in branch]
        lines += ["else:", "    pass", "return 0"]
        source += machine(number, lines)

    for slot in range(10):
        for save, number in [(True, 920 + slot), (False, 940 + slot)]:
            request = SAVE + slot if save else LOAD + slot
            lines = [f"SetEventFlag({DONE}, FlagState.Off)", f"SetEventFlag({ERROR}, FlagState.Off)",
                     f"SetEventFlag({request}, FlagState.On)",
                     f"assert GetEventFlag({DONE}) or GetEventFlag({ERROR}) or GetCurrentStateElapsedTime() > 15",
                     f"if GetEventFlag({DONE}):", f"    assert {SCRIPT}_x{910 if save else 911}()",
                     "else:", f"    SetEventFlag({request}, FlagState.Off)", f"    assert {SCRIPT}_x914()", "return 0"]
            source += machine(number, lines)
    for number, text_id in [(910, TEXT + 5), (911, TEXT + 6), (912, TEXT + 7), (914, TEXT + 9)]:
        source += machine(number, [f"OpenGenericDialog(DialogBoxType.CenterBottom1, {text_id}, DialogResult.Left, DialogBoxStyle.OrnateNoOptions, 1)",
                                   "assert not CheckSpecificPersonGenericDialogIsOpen(0)", "return 0"])
    return source


if __name__ == "__main__":
    source, destination = map(Path, sys.argv[1:])
    destination.write_text(patch(source.read_text(encoding="utf-8-sig")), encoding="utf-8")
