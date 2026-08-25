#!/usr/bin/env python3
"""Vision board for vigia #318: renders a vigia-like frame in candidate looks.

Variants: a=current  b=delta-washes  c=b+gradients  d=c+chrome  e=d with octant sparklines
Honest mock: real terminal, real font, real ANSI. Content mirrors the chaotic lab repo.
"""
import sys

V = sys.argv[1] if len(sys.argv) > 1 else "a"
W = 150

def fg(h): return f"\x1b[38;2;{int(h[1:3],16)};{int(h[3:5],16)};{int(h[5:7],16)}m"
def bg(h): return f"\x1b[48;2;{int(h[1:3],16)};{int(h[3:5],16)};{int(h[5:7],16)}m"
R = "\x1b[0m"; B = "\x1b[1m"; DIM = "\x1b[2m"
def lerp(a, b, t):
    return "#%02x%02x%02x" % tuple(int(int(a[i:i+2],16)+(int(b[i:i+2],16)-int(a[i:i+2],16))*t) for i in (1,3,5))

# palette
TXT="#c9d1d9"; MUT="#8b949e"; CY="#39c5cf"; GRN="#3fb950"; RED="#f85149"; YEL="#d29922"
HDR="#58a6ff"
if V=="a":
    AB="#12261a"; ABW=AB; RB="#2d1418"; RBW=RB           # flat slab, no word emph
else:
    AB="#0c1f10"; ABW="#1c4d24"; RB="#230f12"; RBW="#5c1a22"  # delta: calm wash + hot word
GUT_A = lerp(AB,"#000000",0.35); GUT_R = lerp(RB,"#000000",0.35)

SEED=0
def spark(v):
    global SEED; SEED=(SEED+3)%8
    ramp="▁▂▃▄▅▆▇█"
    if V=="e": ramp="𜺨𜺣𜴀𜴆𜴡𜵈𜵟𜶮"  # octant-ish ladder (2x4 cells; terminal-rendered)
    vals=([0,1,2,4,6,7,5,3]*3)[SEED:SEED+8]
    out=""
    for i,x in enumerate(vals):
        if V in "cde":
            c = lerp("#0e4429","#39d353", x/7)
            out += fg(c)+ramp[x]
        else:
            out += fg(CY)+ramp[x]
    return out+R

def heat():
    if V in "cde":
        cells=[lerp("#0e4429","#39d353",i/11) for i in range(6)]+[lerp("#9e6a03","#d29922",i/5) for i in range(3)]+[lerp("#67060c","#f85149",i/2) for i in range(3)]
        return "".join(fg(c)+"■" for c in cells)+R
    return fg(GRN)+"■■■■■■"+fg(YEL)+"■■■"+fg(RED)+"■■■"+R

ICON = {"rs": (fg("#dea584"),""), "md": (fg(HDR),""), "bin": (fg(MUT),"")}
def icon(name):
    if V not in "de": return ""
    ext = name.rsplit(".",1)[-1]
    c,g = ICON.get(ext, (fg(MUT),""))
    return c+g+R+" "

def pad(s, n):  # pad by printable length
    import re; ln = len(re.sub(r"\x1b\[[0-9;:]*m","",s)); return s+" "*max(0,n-ln)

def hdr():
    if V in "de":
        s  = bg("#1f6f3f")+fg("#e6edf3")+B+" repo "+R+bg("#274d33")+fg("#1f6f3f")+""+R
        s += bg("#274d33")+fg("#c9d1d9")+" master "+R+bg("#173042")+fg("#274d33")+""+R
        s += bg("#173042")+fg("#c9d1d9")+" 19 changed · 4 staged "+R+fg("#173042")+""+R
        return s+pad("",W-46)+fg(MUT)+"watching  "+R
    return fg(TXT)+B+" repo"+R+fg(MUT)+" · master · 19 changed · 4 staged"+pad("",W-40)+fg(MUT)+"watching  "+R

def band():
    out="  "
    for i in range(48):
        h=[0,0,1,1,2,3,4,3,2,1,1,0][i%12]
        c = lerp("#0e4429","#39d353",h/7) if V in "cde" else CY
        out+=fg(c)+"▁▂▃▄▅▆▇█"[h]
    return out+R

def filerow(sig,sigc,name,plus,minus,cur=False):
    star = fg(CY)+"▸"+R if cur else " "
    nm = (B if cur else "")+fg(TXT)+name+R
    row=f" {star}{fg(sigc)}{sig}{R} {icon(name)}{nm}"
    row=pad(row, 52)+heat()+"  "+spark(3)+"  "
    row=pad(row, 76)+fg(GRN)+f"{plus:>5}"+R+" "+fg(RED)+f"{minus:>4}"+R
    return row

def diffline(no, kind, txt, words=None):
    # kind: ctx/add/rem ; words: list of (start,end) word-emph spans
    num = f"{no:>4} "
    if kind=="ctx":
        return fg(MUT)+num+R+"  "+fg(TXT)+txt+R
    lb, wb, gut, sign, sc = (AB, ABW, GUT_A, "+", GRN) if kind=="add" else (RB, RBW, GUT_R, "-", RED)
    if V=="a":
        body = fg(TXT)+txt
        return bg(lb)+fg(sc)+num+sign+" "+body+pad("",W-8-len(txt))+R
    out = bg(gut)+fg(sc)+num+R+bg(lb)+fg(sc)+sign+" "+R
    pos=0; body=""
    for s,e in (words or []):
        body += bg(lb)+fg(TXT)+txt[pos:s]+R+bg(wb)+fg("#e6edf3")+txt[s:e]+R; pos=e
    body += bg(lb)+fg(TXT)+txt[pos:]+pad("",W-8-len(txt))+R
    return out+body

print(hdr())
if V in "cde": print(band())
rows=[("M",YEL,"core/engine/diff.rs",59,-35,False),("M",YEL,"core/engine/status.rs",117,-2,False),
      ("A",GRN,"core/store/fresh_cache.rs",70,0,False),("D",RED,"src/dying.rs",0,-90,False),
      ("M",YEL,"src/net/proto.rs",113,-83,True),("M",YEL,"src/render/frame.rs",97,-12,False)]
for r in rows: print(filerow(*r))
print(fg(MUT)+" "+"─"*(W-2)+R)
print(" "+fg(YEL)+"M"+R+" "+icon("proto.rs")+B+fg(TXT)+"src/net/proto.rs"+R)
print(fg(HDR)+"  @@ -4,8 +4,8 @@"+R)
print(diffline(4,"ctx","fn wire_4() -> usize { 4 * 2 }   // body of proto line 4"))
print(diffline(5,"rem","fn proto_5() -> usize { 5 * 2 }  // body of proto line 5",[(3,10)]))
print(diffline(5,"add","fn wire_5() -> usize { 51 * 2 }  // body of proto line 5",[(3,9),(23,25)]))
print(diffline(6,"ctx","fn wire_6() -> usize { 6 * 2 }   // body of proto line 6"))
print(diffline(7,"ctx","fn wire_7() -> usize { 7 * 2 }   // body of proto line 7"))
print(fg(HDR)+"  @@ -447,3 +447,9 @@"+R)
print(diffline(447,"ctx","fn proto_447() -> usize { 447 * 2 }"))
for i in range(448,454):
    print(diffline(i,"add",f"fn added_{i}() -> usize {{ {i} }}  // new"))
if V in "de":
    # the sheet, rounded, title spliced into the border (btop technique)
    x0=W-46
    print(pad("",x0)+fg(MUT)+"╭─"+fg(CY)+"┐"+R+B+fg(TXT)+" gestures "+R+fg(CY)+"┌"+fg(MUT)+"─"*28+"╮"+R)
    for k,v in [("j k ↓ ↑","scroll a row"),("n → / p ←","next / previous file"),("a","staged changes"),("?","this sheet")]:
        print(pad("",x0)+fg(MUT)+"│ "+R+fg(CY)+pad(k,12)+R+fg(MUT)+pad(v,28)+"│"+R)
    print(pad("",x0)+fg(MUT)+"╰"+"─"*42+"╯"+R)
if V in "de":
    s  = pad("",1)+bg("#173042")+fg(TXT)+" q quit · f follow · ? keys "+R+fg("#173042")+""+R
    s  = pad(s, W-30)+fg("#274d33")+""+R+bg("#274d33")+fg(TXT)+" 2.4ms "+R+bg("#1f6f3f")+fg("#e6edf3")+B+" follow ▶ "+R
    print(s)
else:
    print(pad(fg(MUT)+" q quit · f follow · ? keys"+R, W-24)+fg(MUT)+"2.4ms frame  follow ▶"+R)
