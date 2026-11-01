r"""OFF-3 (2026-10-22): reproduce the static findings of research/OFF-3-bots.md on nightreign.exe 1.3.3.0.
Run: %LOCALAPPDATA%\Programs\Python\Python312\python.exe off3_bots.py  (needs capstone + pefile; nrimg.py next to it caches the
mapped image, ~130 MB, in %TEMP%)."""
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from nrimg import Img

I = Img()
TX = [(va, va + sz) for (n, va, sz) in I.secs if n == '.text']
BOTMAN_SLOT = 0x3C0B518          # CSNpcBotMan (singleton log, FD4)
print('CSNpcBotMan slot refs:', [hex(a) for a, _ in I.ripxrefs(BOTMAN_SLOT, TX)])
# EMEVD bank dispatcher: edx = bank - 2001, table at 0x587844 -> bank 2003 handler 0x58cba0
print('bank 2003 handler:', hex(struct.unpack_from('<i', I.img, 0x587844 + 4 * 2)[0]))
# bank 2003 handler: ecx = instruction id - 3, jump table at 0x5901e0
for iid in (97, 98):
    print(f'2003[{iid}] case ->', hex(struct.unpack_from('<i', I.img, 0x5901e0 + 4 * (iid - 3))[0]))
print('2003[97] thunk (ecx = int arg -> CSNpcBotMan +0x38cf70):'); I.dis(0x607950, 5)
print('2003[98] thunk (-> CSNpcBotMan +0x38d110):'); I.dis(0x60b090, 4)
print('solo gate inside 0x38cf70:'); I.dis(0x38cf87, 3); I.dis(0x6af500, 3)
