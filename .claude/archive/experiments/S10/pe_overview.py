"""S10: read-only overview of a PE (sections, entropy, imports, exports). Prints names/numbers only; no code bytes."""
import sys, pefile, math, collections
p = pefile.PE(sys.argv[1], fast_load=False)
print("ImageBase", hex(p.OPTIONAL_HEADER.ImageBase), "Entry", hex(p.OPTIONAL_HEADER.AddressOfEntryPoint), "TimeDateStamp", hex(p.FILE_HEADER.TimeDateStamp))
for s in p.sections:
    d = s.get_data(); c = collections.Counter(d); n = len(d) or 1
    ent = -sum(v/n*math.log2(v/n) for v in c.values())
    print(f"{s.Name.rstrip(b'\0').decode(errors='replace'):10} va={s.VirtualAddress:#x} vsz={s.Misc_VirtualSize:#x} raw={s.SizeOfRawData:#x} ent={ent:.2f} ch={s.Characteristics:#x}")
if hasattr(p, 'DIRECTORY_ENTRY_IMPORT'):
    for e in p.DIRECTORY_ENTRY_IMPORT:
        names = [ (i.name.decode() if i.name else f"#{i.ordinal}") for i in e.imports]
        print("IMPORT", e.dll.decode(), len(names), ", ".join(names[:400]))
if hasattr(p, 'DIRECTORY_ENTRY_EXPORT'):
    for x in p.DIRECTORY_ENTRY_EXPORT.symbols:
        print("EXPORT", x.name, hex(x.address))
print("TLS", hasattr(p,'DIRECTORY_ENTRY_TLS'))
print("pdata entries", len(getattr(p,'DIRECTORY_ENTRY_EXCEPTION',[]) or []))
