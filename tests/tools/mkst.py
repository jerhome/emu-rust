import struct,sys
# usage: mkst.py out.st PROG.PRG   -> 720K FAT12 floppy with \AUTO\PROG.PRG
out,prg=sys.argv[1],sys.argv[2]; data=open(prg,'rb').read()
SS=512;SPC=2;RES=1;NF=2;ROOT=112;TOT=1440;SPF=3
img=bytearray(TOT*SS)
struct.pack_into('<HBHBHHBHHHII',img,11,SS,SPC,RES,NF,ROOT,TOT,0xF9,SPF,9,2,0,0)
img[0:3]=b'\xeb\x3c\x90';img[3:11]=b'MSDOS5.0';img[510:512]=b'\x55\xaa'
fat=[0xFF9,0xFFF]+[0]*(TOT//SPC)
cl=SPC*SS
def alloc(buf):
    n=max(1,-(-len(buf)//cl)); first=None;prev=None
    for _ in range(n):
        c=next(i for i in range(2,len(fat)) if fat[i]==0)
        if prev: fat[prev]=c
        else: first=c
        prev=c; fat[c]=0xFFF
    chain=[];c=first
    while True:
        chain.append(c)
        if fat[c]==0xFFF:break
        c=fat[c]
    return first,chain
rootoff=(RES+NF*SPF)*SS; dataoff=rootoff+ROOT*32
def put(chain,buf):
    for i,c in enumerate(chain):
        o=dataoff+(c-2)*cl; ch=buf[i*cl:(i+1)*cl]; img[o:o+len(ch)]=ch
def ent(name,attr,first,size):
    return name.ljust(11).encode()+bytes([attr])+bytes(10)+struct.pack('<HHHI',0,0x5A21,first,size)
f,ch=alloc(data);put(ch,data)
d,dch=alloc(bytes(cl))
name=prg.split('/')[-1].upper().replace('.PRG','').ljust(8)+'PRG'
sub=ent('.',0x10,d,0)+ent('..',0x10,0,0)+ent(name,0x20,f,len(data))
put(dch,sub)
img[rootoff:rootoff+32]=ent('AUTO',0x10,d,0)
for k in range(NF):
    o=(RES+k*SPF)*SS
    for i in range(0,len(fat)):
        pass
    b=bytearray(SPF*SS)
    for i,v in enumerate(fat[:SPF*SS*2//3]):
        p=i*3//2
        if i%2==0: b[p]|=v&0xFF;b[p+1]|=(v>>8)&0xF
        else: b[p]|=(v&0xF)<<4;b[p+1]=(v>>4)&0xFF
    img[o:o+len(b)]=b
open(out,'wb').write(img)
