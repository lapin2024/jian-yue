import hashlib,json,re,struct,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
reader=ROOT/'tools/llvm-mingw-20261006-ucrt-x86_64/bin/llvm-readelf.exe'
obj=ROOT/'tools/llvm-mingw-20261006-ucrt-x86_64/bin/llvm-readobj.exe'
items=[('windows-x64','target/release/jian-yue.exe','PE',0x8664,True),('windows-x86','target/i686-pc-windows-gnu/release/jian-yue.exe','PE',0x14c,True),('windows-arm64','target/aarch64-pc-windows-gnullvm/release/jian-yue.exe','PE',0xaa64,False),('linux-x64','target/x86_64-unknown-linux-gnu/release/jian-yue','ELF',62,False),('linux-x86','target/i686-unknown-linux-gnu/release/jian-yue','ELF',3,False),('linux-arm64','target/aarch64-unknown-linux-gnu/release/jian-yue','ELF',183,False)]
out=ROOT/'docs/verification';out.mkdir(exist_ok=True)
records=[]
for name,location,kind,expected,runtime in items:
    f=ROOT/location;bytes=f.read_bytes()
    if kind=='PE':
        pe=struct.unpack_from('<I',bytes,0x3c)[0]
        assert bytes[pe:pe+4]==b'PE\0\0'
        machine=struct.unpack_from('<H',bytes,pe+4)[0]
        details=subprocess.check_output([str(obj),'--file-headers','--coff-imports',str(f)],text=True,encoding='utf-8',errors='replace')
    else:
        assert bytes[:4]==b'\x7fELF'
        machine=struct.unpack_from('<H',bytes,18)[0]
        details=subprocess.check_output([str(reader),'--file-header','--dynamic','--version-info',str(f)],text=True,encoding='utf-8',errors='replace')
    assert machine==expected,(name,machine,expected)
    # Keep host-specific paths out of the public report.
    details=details.replace(str(f),location)
    (out/(name+'-headers.txt')).write_text(details,'utf-8')
    versions=[tuple(map(int,v.split('.'))) for v in re.findall(r'Name: GLIBC_(\d+(?:\.\d+)+)',details)]
    glibc='.'.join(map(str,max(versions))) if versions else None
    if kind=='ELF':assert max(versions)<=(2,28),(name,glibc)
    records.append({'platform':name,'format':kind,'machine':machine,'size':len(bytes),'sha256':hashlib.sha256(bytes).hexdigest(),'compiled':True,'runtime_tested':runtime,'glibc_max_required_version':glibc})
(out/'artifacts.json').write_text(json.dumps({'date':'2026-10-10','version':'0.3.0','artifacts':records},ensure_ascii=False,indent=2),'utf-8')
for name in ['windows-x64','windows-x86']:
    src=ROOT/'tmp/verification'/name/'cli-checks.json'
    data=json.loads(src.read_text('utf-8'));data['binary']=name+'/简阅.exe'
    (out/(name+'-cli-checks.json')).write_text(json.dumps(data,ensure_ascii=False,indent=2),'utf-8')
print(json.dumps(records,indent=2))
