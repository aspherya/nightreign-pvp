r"""M5-1 (2026-10-22): reproduce the static findings on the play-area clock in nightreign.exe 1.3.3.0 (research/M5-no-boss-nights.md,
MEMORY_MAP §16). Run: %LOCALAPPDATA%\Programs\Python\Python312\python.exe m5_clock.py  (uses ../OFF-3/nrimg.py, capstone + pefile)."""
import os, struct, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'OFF-3'))
from nrimg import Img

I = Img()
case = struct.unpack_from('<i', I.img, 0x5901e0 + 4 * (89 - 3))[0]
print('2003[89] ResetTimeOfDay case', hex(case), '-> 0x5fea00 -> WorldPlayAreaManager (slot 0x3C13158) method 0x68c1b0'); I.dis(case, 7)
print('\nreset request (hours/minutes args unused; sets +0x135..+0x13C, +0x140 = 1):'); I.dis(0x68c2e0, 12)
print('\nIF Play Area Current Time In Range helper 0x603070: reads DLDateTime [mgr+0xF8], packed half [mgr+0x100]:'); I.dis(0x603077, 6); I.dis(0x6030d9, 12)
print('\nper-frame clock update 0x698430 (dt * speed [+0x118]); cap at 23:59:59 (0x6985e3); set-time mailbox +0x124/+0x128/+0x12C:')
I.dis(0x6985d9, 10); I.dis(0x69860a, 20)
print('\nsetter 0x68ff30(mgr, h, m, s): adds the differences to the DLDateTime at +0xF8:'); I.dis(0x68ff30, 14)
