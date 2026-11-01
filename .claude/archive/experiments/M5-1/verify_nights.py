r"""M5-3 (2026-10-22): check the built no-boss-night event files against the vanilla 1.03.3 ones: only the intended events changed.
Usage: python verify_nights.py <vanilla event dir with .emevd> <built common.emevd> <built common_func.emevd>
(decompress the .dcx files first, e.g. WitchyBND --passive --silent --dcx <files>). Uses emdec.py next to this file."""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emdec

van, common, func = sys.argv[1:4]
REPLACED = {90055000, 90055001, 90065251, 90065252, 90065253, 90035000, 90015442}
ok = True
for name, built, allowed in (('common', common, {0}), ('common_func', func, REPLACED)):
    a, b = emdec.events(os.path.join(van, f'{name}.emevd')), emdec.events(built)
    changed = {k for k in a if a[k] != b.get(k)}
    added = sorted(k for k in b if k not in a)
    print(f'{name}: changed {sorted(changed)}, added {added}')
    ok &= changed <= allowed and (name != 'common_func' or changed == REPLACED)
print('OK' if ok else 'UNEXPECTED CHANGES')
sys.exit(0 if ok else 1)
