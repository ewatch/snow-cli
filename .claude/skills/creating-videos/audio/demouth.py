#!/usr/bin/env python3
"""
De-mouth a voice WAV (remove lip smacks / clicks) in two complementary passes:
  1. gentle adeclick (threshold high) for clicks during/attached to speech
  2. mute short non-silent "islands" that sit between two real pauses
     -> these isolated short transients in gaps are mouth smacks/clicks.
     Muting is safe because the island is flanked by silence (no speech to harm);
     the mute edges land inside the surrounding silence so no new click is created.

QA tip: build the residual (orig - processed) by phase-inverting one and mixing;
it must contain transients ONLY (no speech), and duration/peak/RMS must be unchanged.

Usage: demouth.py INPUT.wav OUTPUT.wav [--report]
"""
import subprocess, sys, re, argparse

NOISE_DB      = -40.0   # silencedetect threshold
SIL_MINDUR    = 0.08    # min silence duration to register a pause (s)
MAX_ISLAND    = 0.18    # islands shorter than this between pauses = mouth sound (s)
MIN_FLANK     = 0.05    # require this much silence on EACH side of the island (s)
EDGE_MARGIN   = 0.020   # extend mute this far into the surrounding silence (s)
DECLICK_THR   = 6       # adeclick threshold (gentle; ffmpeg default 2 over-processes)

def run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True)

def get_silences(path):
    txt = run(["ffmpeg","-hide_banner","-i",path,"-af",
               f"silencedetect=noise={NOISE_DB}dB:d={SIL_MINDUR}","-f","null","-"]).stderr
    starts = [float(m) for m in re.findall(r"silence_start:\s*([0-9.]+)", txt)]
    ends   = [float(m) for m in re.findall(r"silence_end:\s*([0-9.]+)", txt)]
    return list(zip(starts, ends))

def get_duration(path):
    return float(run(["ffprobe","-v","error","-show_entries","format=duration",
                      "-of","default=nk=1:nw=1", path]).stdout.strip())

def find_smack_islands(sil):
    islands = []
    for i in range(len(sil)-1):
        s_end, s_next = sil[i][1], sil[i+1][0]
        isl_len = s_next - s_end
        flank_before = sil[i][1]   - sil[i][0]
        flank_after  = sil[i+1][1] - sil[i+1][0]
        if 0 < isl_len <= MAX_ISLAND and flank_before >= MIN_FLANK and flank_after >= MIN_FLANK:
            a = max(s_end - min(EDGE_MARGIN, flank_before*0.5), sil[i][0])
            b = min(s_next + min(EDGE_MARGIN, flank_after*0.5), sil[i+1][1])
            islands.append((a, b, isl_len))
    return islands

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("inp"); ap.add_argument("out")
    ap.add_argument("--report", action="store_true")
    a = ap.parse_args()

    islands = find_smack_islands(get_silences(a.inp))
    print(f"smack-islands={len(islands)}")
    for (s,e,l) in islands:
        print(f"  mute {s:7.3f}-{e:7.3f}  (island {l*1000:5.1f} ms)")
    if a.report:
        return

    if islands:
        cond = "+".join(f"between(t,{s:.4f},{e:.4f})" for (s,e,_) in islands)
        af = f"adeclick=threshold={DECLICK_THR},volume=enable='{cond}':volume=0"
    else:
        af = f"adeclick=threshold={DECLICK_THR}"
    p = run(["ffmpeg","-hide_banner","-y","-i",a.inp,"-af",af,a.out])
    if p.returncode != 0:
        sys.stderr.write(p.stderr[-2000:]); sys.exit(1)
    print(f"wrote {a.out}")

if __name__ == "__main__":
    main()
