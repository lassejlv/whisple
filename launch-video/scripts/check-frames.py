#!/usr/bin/env python3
"""Flags single-frame render glitches (unpainted tiles, stray rectangles).

A frame is suspicious when some block of it disagrees with both neighbours
while those neighbours agree with each other. Real motion and cuts change the
neighbours too, so they are not reported.

usage: check-frames.py video.mp4 [threshold]
"""
import subprocess
import sys

import numpy as np

W, H, BLOCK = 480, 270, 10

path = sys.argv[1]
threshold = float(sys.argv[2]) if len(sys.argv) > 2 else 5.0

raw = subprocess.run(
    ["ffmpeg", "-loglevel", "error", "-i", path, "-vf", f"scale={W}:{H}",
     "-f", "rawvideo", "-pix_fmt", "gray", "-"],
    check=True, capture_output=True,
).stdout
frames = np.frombuffer(raw, np.uint8).reshape(-1, H, W).astype(np.float32)


def blocks(img):
    h, w = H // BLOCK * BLOCK, W // BLOCK * BLOCK
    return img[:h, :w].reshape(h // BLOCK, BLOCK, w // BLOCK, BLOCK).mean(axis=(1, 3))


bad = 0
for n in range(1, len(frames) - 1):
    prev, cur, nxt = frames[n - 1], frames[n], frames[n + 1]
    odd = blocks(np.abs(cur - (prev + nxt) / 2)) - blocks(np.abs(prev - nxt))
    worst = float(odd.max())
    if worst > threshold:
        y, x = np.unravel_index(odd.argmax(), odd.shape)
        print(f"frame {n}: block deviation {worst:.1f} at ~({x * BLOCK * 4}, {y * BLOCK * 4})")
        bad += 1

print(f"checked {len(frames)} frames, {bad} suspicious")
sys.exit(1 if bad else 0)
