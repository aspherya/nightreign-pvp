"""Kill a Dormant-Power boss on instance 1 with a real hit and report where the orb is.
Usage: python orbkill.py            (both games already in an expedition)
"""
import json, re, subprocess, sys, time

NRO = r"C:\Users\Leonardo\source\repos\nightreign-pvp\.claude\worktrees\happy-goldstine-746484\target\release\nro.exe"
NEVER = ("46400010", "75700010")  # Nightlord-class: killing them ends the expedition


def cmd(inst, line):
    out = subprocess.run([NRO, "--i", str(inst), "cmd", line], capture_output=True, text=True).stdout
    try:
        return json.loads(out)
    except Exception:
        return out


def keys(inst, seq):
    subprocess.run([NRO, "--i", str(inst), "keys", seq], capture_output=True, text=True)


def pick():
    for n in range(0, 80):
        t = cmd(1, f"boss_info {n}")
        if not isinstance(t, str) or "error" in t:
            continue
        m = re.search(r"npc (\d+) .*rewardItemLot_2=(-?\d+)", t)
        if not m:
            continue
        npc, lot = m.group(1), int(m.group(2))
        if npc in NEVER:
            continue
        if lot == 6110000:  # the Dormant Power lot seen on Erdtree Avatar
            return n, npc, t
    return None


p = pick()
print("pick", p)
if not p:
    sys.exit(1)
n, npc, info = p
lst = cmd(1, "boss_list")
m = re.search(r"#%d (0x[0-9A-Fa-f]{16}) npc %s" % (n, npc), lst)
h = m.group(1)
print("handle", h)
print(cmd(1, "god 1"))
print(cmd(1, f"boss_go {h}"))
time.sleep(4)
print(cmd(1, f"boss_hp {h} 1"))
time.sleep(1)
for i in range(8):
    keys(1, "click 0.5 0.5; wait 700ms; click 0.5 0.5; wait 700ms; click 0.5 0.5; wait 1s")
    time.sleep(1)
    lst = cmd(1, "boss_list")
    if h[2:] not in str(lst):
        pass
print(cmd(1, "pos"))
