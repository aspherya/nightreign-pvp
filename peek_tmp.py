import subprocess,re,struct,sys,json
N='C:/Users/Leonardo/source/repos/nightreign-pvp/target/release/nro.exe'
def peek(addr,n):
    out=subprocess.run([N,'cmd',f'peek {addr} {n}'],capture_output=True,text=True).stdout
    t=json.loads(out) if out.strip().startswith('"') else out
    qs={}
    for m in re.finditer(r'\+([0-9A-F]+): ([0-9A-F]{16}) ([0-9A-F]{16})',t):
        o=int(m.group(1),16); qs[o]=int(m.group(2),16); qs[o+8]=int(m.group(3),16)
    return qs
if __name__=='__main__':
    a=sys.argv[1]; n=int(sys.argv[2])
    qs=peek(a,n)
    for o in sorted(qs): print(f'+{o:03X} {qs[o]:016X}')
