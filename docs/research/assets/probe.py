#!/usr/bin/env python3
# Visual vocabulary probe for vigia #318. Prints numbered sections; screenshot judges.
import sys
W = lambda s: sys.stdout.write(s)
def rgb(r,g,b,t): return f"\x1b[38;2;{r};{g};{b}m{t}\x1b[0m"
def bg(r,g,b,t): return f"\x1b[48;2;{r};{g};{b}m{t}\x1b[0m"

print("== 1 blocks      ▁▂▃▄▅▆▇█  ▀▄▌▐  ░▒▓")
print("== 2 sextants    " + "".join(chr(c) for c in range(0x1FB00,0x1FB10)) + "  mosaics: " + "".join(chr(c) for c in range(0x1FB3C,0x1FB44)))
print("== 3 octants     " + "".join(chr(c) for c in range(0x1CD00,0x1CD10)))
print("== 4 braille     ⣀⣠⣤⣴⣶⣾⣿  ⡀⡄⡆⡇⣇⣧⣷⣿")
print("== 5 combining   M⃒ (M+20D2)   ≠ (=+0338)   M̶ (M+0336)   a⃝ (a+20DD)   ⃠ (icon+20E0)")
print("== 6 nficons      rust   folder   branch     js   py   db")
print("== 7 powerline   " + bg(31,111,63, rgb(255,255,255," main ")) + rgb(31,111,63,"") + "  round: " + bg(40,80,160, " x ") + rgb(40,80,160,"") + "  " + rgb(200,120,40,"") + bg(200,120,40," y ") + "  flame: " + rgb(120,40,160,""))
# 8: smooth truecolour ramps, fg on full blocks and a bg wash band
ramp = "".join(rgb(int(20+ i*3.9), int(220-i*2.5), int(80+i*1.2), "█") for i in range(60))
wash = "".join(bg(int(15+i*2), int(30+i*1.5), int(60+i*3), " ") for i in range(60))
print("== 8 gradient    " + ramp)
print("                 " + wash)
print("== 9 underline   \x1b[4:1mplain\x1b[0m  \x1b[4:2mdouble\x1b[0m  \x1b[4:3mcurly\x1b[0m  \x1b[4:4mdotted\x1b[0m  \x1b[4:5mdashed\x1b[0m  \x1b[4:3;58:2::255:80:80mred-curly\x1b[0m")
# 10: delta-style diff washes: whole-line tint + brighter word emphasis
print("== 10 diffwash")
print("\x1b[48;2;18;38;24m  + fn " + "\x1b[48;2;30;80;40mrender_frame\x1b[48;2;18;38;24m(&self) -> Frame {" + " "*18 + "\x1b[0m")
print("\x1b[48;2;45;22;24m  - fn " + "\x1b[48;2;96;36;40mdraw_frame\x1b[48;2;45;22;24m(&self) -> Frame {" + " "*23 + "\x1b[0m")
print("== 11 osc8       \x1b]8;;https://github.com/breferrari/vigia\x1b\\vigia on GitHub\x1b]8;;\x1b\\  (hover: link?)")
print()
