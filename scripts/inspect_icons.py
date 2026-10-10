"""Check PE icon resources against the exact supplied PNG bytes."""
from pathlib import Path
import struct,json,sys,hashlib
ROOT=Path(__file__).resolve().parents[1]
def inspect(path):
    data=Path(path).read_bytes();pe=struct.unpack_from('<I',data,0x3c)[0]
    machine,sections=struct.unpack_from('<HH',data,pe+4);opt=pe+24
    opt_size=struct.unpack_from('<H',data,pe+20)[0]
    magic=struct.unpack_from('<H',data,opt)[0];directory=opt+(112 if magic==0x20b else 96)
    resource_rva=struct.unpack_from('<I',data,directory+16)[0]
    mappings=[]
    for i in range(sections):
        at=opt+opt_size+i*40
        virtual_size,virtual_address,raw_size,raw_address=struct.unpack_from('<IIII',data,at+8)
        mappings.append((virtual_address,max(raw_size,virtual_size),raw_address))
    def offset(rva):
        for va,size,raw in mappings:
            if va<=rva<va+size:return raw+rva-va
        raise ValueError(f'RVA {rva} not mapped')
    base=offset(resource_rva)
    def entries(relative):
        at=base+relative;named,ids=struct.unpack_from('<HH',data,at+12)
        return [struct.unpack_from('<II',data,at+16+i*8) for i in range(named+ids)]
    def leaves(relative):
        out=[]
        for name,target in entries(relative):
            if target&0x80000000:out.extend(leaves(target&0x7fffffff))
            else:
                rva,size=struct.unpack_from('<II',data,base+target);at=offset(rva);out.append(data[at:at+size])
        return out
    types={name:target for name,target in entries(0) if not name&0x80000000}
    icons=leaves(types[3]&0x7fffffff);groups=leaves(types[14]&0x7fffffff)
    assert len(icons)==6 and len(groups)==1
    expected={(ROOT/'assets/icons'/f'icon-{size}.png').read_bytes() for size in [32,48,64,96,128,256]}
    assert set(icons)==expected,'Embedded icon artwork differs from supplied PNGs'
    group=groups[0];count=struct.unpack_from('<H',group,4)[0];sizes=[]
    for i in range(count):
        width=group[6+i*14];sizes.append(width or 256)
    assert sorted(sizes)==[32,48,64,96,128,256]
    versions=leaves(types[16]&0x7fffffff)
    assert any('简阅'.encode('utf-16le') in value for value in versions),'Product name is not embedded in version resources'
    return {'product_name':'简阅','machine':machine,'icon_count':len(icons),'sizes':sorted(sizes),'original_png_bytes_preserved':True}
if __name__=='__main__':
    results={}
    for name,path in [('windows-x64','target/release/jian-yue.exe'),('windows-x86','target/i686-pc-windows-gnu/release/jian-yue.exe'),('windows-arm64','target/aarch64-pc-windows-gnullvm/release/jian-yue.exe')]:
        results[name]=inspect(ROOT/path)
    out=ROOT/'docs/verification/icon-resources.json';out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps(results,indent=2),'utf-8');print(json.dumps(results,indent=2))
