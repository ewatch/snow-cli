#!/usr/bin/env python3
"""
Lay a looped, sidechain-ducked music bed under a finished VO, with an intro
lead-in, an outro tail, and optional "breath" moments (extend a natural pause
and let the bed bloom up in the gap).

  make_music_tracks.py VO.wav OUT.wav MUSIC.wav \
      [--breaths "46.8,325.3"] [--pad 2.5] [--intro 3] [--outro 3] \
      [--bed-mean -31] [--voice-gain 0]

MUSIC is looped (-stream_loop) to length. To avoid an audible loop seam, pass a
PRE-CROSSFADED loop, e.g.:
  ffmpeg -i m.wav -i m.wav -i m.wav -filter_complex \
    "[0][1]acrossfade=d=4[a];[a][2]acrossfade=d=4" bed_loop.wav

--breaths are MIDPOINTS of existing pauses in ORIGINAL VO seconds; each is
widened by --pad and the bed blooms (un-ducks) in the gap. --voice-gain (dB,
negative) lowers the voice relative to the bed. Normalize the WHOLE mix to
delivery loudness afterward (loudnorm I=-16) to preserve balance at spec.
"""
import subprocess, sys, argparse, re

SR = 44100

def run(cmd): return subprocess.run(cmd, capture_output=True, text=True)
def dur(p):  return float(run(["ffprobe","-v","error","-show_entries","format=duration",
                               "-of","default=nk=1:nw=1",p]).stdout.strip())
def mean_db(p):
    m = re.search(r"mean_volume:\s*(-?[0-9.]+) dB",
                  run(["ffmpeg","-hide_banner","-i",p,"-af","volumedetect","-f","null","-"]).stderr)
    return float(m.group(1))

def build_extended(vo, out, breaths, pad, intro, outro):
    D = dur(vo); pts = sorted(p for p in breaths if 0 < p < D)
    cuts = [0.0] + pts + [D]; parts = []; f = []
    f.append(f"aevalsrc=0:d={intro}:s={SR}:c=mono[i]"); parts.append("[i]")
    for k in range(len(cuts)-1):
        a,b = cuts[k], cuts[k+1]
        f.append(f"[0:a]atrim={a}:{b},asetpts=N/SR/TB[s{k}]"); parts.append(f"[s{k}]")
        if k < len(pts):
            f.append(f"aevalsrc=0:d={pad}:s={SR}:c=mono[p{k}]"); parts.append(f"[p{k}]")
    f.append(f"aevalsrc=0:d={outro}:s={SR}:c=mono[o]"); parts.append("[o]")
    f.append("".join(parts)+f"concat=n={len(parts)}:v=0:a=1[vx]")
    run(["ffmpeg","-hide_banner","-y","-i",vo,"-filter_complex",";".join(f),"-map","[vx]",out])
    blooms = [(intro+p+pad*k, intro+p+pad*k+pad) for k,p in enumerate(pts)]
    return dur(out), blooms

def mix(vo_ext, music, out, bed_mean, voice_gain, intro, outro):
    total = dur(vo_ext); g = round(bed_mean - mean_db(music), 2)
    fc = (f"[0:a]aresample={SR},volume={voice_gain}dB,pan=stereo|c0=c0|c1=c0,asplit=2[vo][vosc];"
          f"[1:a]aresample={SR},volume={g}dB,atrim=0:{total},"
          f"afade=t=in:st=0:d={max(intro-0.5,0.1)},afade=t=out:st={total-outro-1}:d={outro+1}[mus];"
          f"[mus][vosc]sidechaincompress=threshold=0.02:ratio=10:attack=5:release=400[duck];"
          f"[vo][duck]amix=inputs=2:normalize=0,alimiter=limit=0.95[out]")
    p = run(["ffmpeg","-hide_banner","-y","-stream_loop","-1","-i",vo_ext,
             "-stream_loop","-1","-i",music,"-filter_complex",fc,"-map","[out]","-t",str(total),out])
    if p.returncode: sys.stderr.write(p.stderr[-1500:]); sys.exit(1)
    return total, g

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("vo"); ap.add_argument("out"); ap.add_argument("music")
    ap.add_argument("--breaths", default="", help="comma-separated pause midpoints (orig VO seconds)")
    ap.add_argument("--pad", type=float, default=2.5)
    ap.add_argument("--intro", type=float, default=3.0)
    ap.add_argument("--outro", type=float, default=3.0)
    ap.add_argument("--bed-mean", type=float, default=-31.0)
    ap.add_argument("--voice-gain", type=float, default=0.0)
    ap.add_argument("--vo-ext", default="vo_extended.wav")
    a = ap.parse_args()
    breaths = [float(x) for x in a.breaths.split(",") if x.strip()]
    tot, blooms = build_extended(a.vo, a.vo_ext, breaths, a.pad, a.intro, a.outro)
    print(f"extended VO: {tot:.2f}s")
    for s,e in blooms:
        print(f"  bloom {int(s//60)}:{s%60:05.2f}-{int(e//60)}:{e%60:05.2f}")
    total, g = mix(a.vo_ext, a.music, a.out, a.bed_mean, a.voice_gain, a.intro, a.outro)
    print(f"wrote {a.out} (len {total:.2f}s, bed {g}dB, voice {a.voice_gain}dB)")
