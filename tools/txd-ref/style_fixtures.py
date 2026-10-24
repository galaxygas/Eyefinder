"""Synthetic DMA retail styles: CityScape 12.10 §§3.3–3.8.
Five tiles cross a strip boundary and require three padding tiles; two sprites
cross a page boundary in the sprite store. No original game data.
"""
import os, struct, json, hashlib

def generate(out):
    for g24 in (False, True):
        tiles = bytearray(32768)
        for t in range(5):
            for y in range(64):
                for x in range(64):
                    tiles[(t//4)*16384+y*256+(t%4)*64+x] = (t*41+x*3+y*7)%256
        sprites=bytearray(131072)
        info=bytearray()
        for n, start in enumerate((254, 65536+513)):
            w,h=2,3
            for y in range(h):
                for x in range(w): sprites[start+y*256+x]=(n*50+y*3+x)%256
            delta=struct.pack('<HB', 256, 2)+bytes([0,233])
            offset=100000+n*16
            sprites[offset:offset+len(delta)]=delta
            info+=struct.pack('<BBBBH',w,h,1,0,w*h)
            if g24: info+=struct.pack('<H',n)
            info+=struct.pack('<IHI',start,len(delta),offset)
        if g24:
            palette=bytearray(131072)
            # Deliberately select physical palette 65, exercising the second CLUT page.
            ids=[65 if v%2 else 2 for v in range(22)]
            for p in (2,65):
                for c in range(256):
                    off=(p//64)*65536+c*256+(p%64)*4
                    palette[off:off+4]=bytes([(c+p)%256,(c*3+p)%256,(255-c+p)%256,0])
            index=struct.pack('<22H',*ids)
            sizes=[336,4096,4096,12288,1,66560,20*1024,2*1024,0,0,len(index),0,0,len(info),len(sprites),42]
            sections=[tiles,b'\0',palette,index,info,sprites,bytes(42)]
            name='style.g24'
        else:
            palette=bytes(v for c in range(256) for v in (c,(c*3)%256,255-c))
            remaps=bytes((c+r)%256 for r in range(256) for c in range(256))
            index=bytes([0,1,2,3])*256
            sizes=[325,4096,4096,12288,1,len(palette),len(remaps),len(index),0,0,len(info),len(sprites),42]
            sections=[tiles,b'\0',palette,remaps,index,info,sprites,bytes(42)]
            name='style.gry'
        open(os.path.join(out,name),'wb').write(struct.pack('<'+'I'*len(sizes),*sizes)+b''.join(sections))

def fnv(data):
    h=0xcbf29ce484222325
    for b in data: h=((h^b)*0x100000001b3)&0xffffffffffffffff
    return f"{h:016x}"

def reference(out):
    # Expected pixels are generated in image order from the semantic pattern above,
    # independently of section offsets, page addressing and Rust decoding.
    result={}
    for g24 in (False,True):
        rows=[]
        def color(i,v=0):
            if g24:
                p=65 if v%2 else 2
                return bytes([(255-i+p)%256,(i*3+p)%256,(i+p)%256,255 if i else 0])
            return bytes([i,(i*3)%256,255-i,255 if i else 0])
        def add(name,w,h,pixels):
            rows.append(dict(name=name,width=w,height=h,fnv=fnv(pixels)))
        for t in range(5):
            group,idx=('side',0) if t==0 else ('lid',0) if t==1 else ('aux',t-2)
            for variant in range(4 if g24 or t==1 else 1):
                pixels=b''.join(color((t*41+x*3+y*7+(variant if not g24 and t==1 else 0))%256,t*4+variant) for y in range(64) for x in range(64))
                add(f'{group}_{idx:04}_palette_{variant}',64,64,pixels)
        for n in range(2):
            values=[(n*50+y*3+x)%256 for y in range(3) for x in range(2)]
            add(f'sprite_{n:04}',2,3,b''.join(color(i,20+n) for i in values))
            values[2:4]=[0,233]
            add(f'sprite_{n:04}_delta_00',2,3,b''.join(color(i,20+n) for i in values))
        result['style.g24' if g24 else 'style.gry']=rows
    open(os.path.join(out,'style_reference.json'),'w').write(json.dumps(result,indent=2)+'\n')

def generate_sty(out):
    # Chunk order deliberately unrelated to processing order; include an unknown chunk.
    tile=bytearray(32768)
    for t in range(8):
        for y in range(64):
            for x in range(64): tile[(t//4)*16384+y*256+(t%4)*64+x]=(t*41+x*3+y*7)%256
    pp=bytearray(131072)
    for p in (2,65):
        for c in range(256):
            off=(p//64)*65536+c*256+(p%64)*4
            pp[off:off+4]=bytes([(c+p)%256,(c*3+p)%256,(255-c+p)%256,0])
    index=struct.pack('<16384H',*[65 if v%2 else 2 for v in range(16384)])
    gfx=bytearray(65536);info=bytearray()
    for n,start in enumerate((254,1024)):
        for y in range(3):
            for x in range(2): gfx[start+y*256+x]=(n*50+y*3+x)%256
        info+=struct.pack('<IBBH',start,2,3,0)
    chunks=[(b'SPRX',info),(b'JUNK',b'ignore me'),(b'PPAL',pp),(b'TILE',tile),
            (b'SPRG',gfx),(b'PALX',index),(b'PALB',struct.pack('<8H',8,2,0,0,0,0,0,0)),
            (b'DELX',struct.pack('<HBBH',1,1,0,5)),(b'DELS',struct.pack('<HB',256,2)+bytes([0,233]))]
    data=b'GBST'+struct.pack('<H',700)+b''.join(k+struct.pack('<I',len(v))+v for k,v in chunks)
    open(os.path.join(out,'style.sty'),'wb').write(data)

def reference_sty(out):
    rows=[]
    def color(i,v):
        p=65 if v%2 else 2
        return bytes([(255-i+p)%256,(i*3+p)%256,(i+p)%256,255 if i else 0])
    def add(name,w,h,pix):rows.append(dict(name=name,width=w,height=h,fnv=fnv(pix)))
    for t in range(8):
        add(f'tile_{t:04}',64,64,b''.join(color((t*41+x*3+y*7)%256,t) for y in range(64) for x in range(64)))
    for n in range(2):
        values=[(n*50+y*3+x)%256 for y in range(3) for x in range(2)]
        add(f'sprite_{n:04}',2,3,b''.join(color(i,8+n) for i in values))
    values[2:4]=[0,233]
    add('sprite_0001_delta_00',2,3,b''.join(color(i,9) for i in values))
    file=os.path.join(out,'style_reference.json');ref=json.load(open(file));ref['style.sty']=rows
    open(file,'w').write(json.dumps(ref,indent=2)+'\n')

def generate_fon(out):
    # Mixed widths, a zero-width spacer, and every palette index.
    data=bytearray([4,16])
    for n,w in enumerate((16,0,3,7)):
        data.append(w)
        data.extend((n*19+y*w+x)%256 for y in range(16) for x in range(w))
    data.extend(c for i in range(256) for c in (i,255-i,(i*13)%256))
    open(os.path.join(out,'images.fon'),'wb').write(data)

def reference_fon(out):
    # Image-order semantic reference, with no FON offset parsing.
    rows=[]
    for n,w in ((0,16),(2,3),(3,7)):
        pixels=bytearray()
        for y in range(16):
            for x in range(w):
                i=(n*19+y*w+x)%256
                pixels.extend((i,255-i,(i*13)%256,255 if i else 0))
        rows.append(dict(name=f'image_{n:03}',width=w,height=16,fnv=fnv(pixels)))
    open(os.path.join(out,'fon_reference.json'),'w').write(json.dumps(rows,indent=2)+'\n')
