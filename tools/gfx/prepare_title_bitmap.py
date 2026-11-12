"""Prepare our own title bitmap after changing the source image. Requires ffmpeg and ffprobe.

Usage: python tools/gfx/prepare_title_bitmap.py IMAGE [--assets gamedata/title]
Normal game-data builds use the committed bitmap and need neither of these image tools.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import zlib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', type=Path)
    parser.add_argument('--assets', type=Path, default=Path(__file__).resolve().parents[2] / 'gamedata/title')
    args = parser.parse_args()
    config_path = args.assets / 'background.json'
    config = json.loads(config_path.read_text())
    probe = subprocess.run(['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries',
                            'stream=width,height,codec_name', '-of', 'json', str(args.image)],
                           check=True, capture_output=True)
    info = json.loads(probe.stdout)['streams'][0]
    width, height = info['width'], info['height']
    if not 0 < width <= 4096 or not 0 < height <= 4096:
        raise ValueError('Use an image no larger than 4096 pixels per side')
    extension = {'webp': 'webp', 'png': 'png', 'mjpeg': 'jpg', 'bmp': 'bmp'}.get(info['codec_name'])
    if extension is None:
        raise ValueError('Use a PNG, WebP, JPEG or BMP image')
    raw = bytearray(subprocess.run(['ffmpeg', '-v', 'error', '-i', str(args.image), '-frames:v', '1',
                                   '-pix_fmt', 'argb', '-f', 'rawvideo', '-'], check=True, capture_output=True).stdout)
    if len(raw) != width * height * 4:
        raise ValueError('Decoded image size differs from its dimensions')
    if any(alpha != 255 for alpha in raw[::4]):
        for offset in range(0, len(raw), 4):
            for channel in range(1, 4):
                raw[offset + channel] = (raw[offset + channel] * raw[offset] + 127) // 255
    source = args.assets / ('background.' + extension)
    if source.resolve() != args.image.resolve():
        shutil.copyfile(args.image, source)
    bitmap = zlib.compress(raw, 9)
    (args.assets / 'background.argb.zlib').write_bytes(bitmap)
    config.update(width=width, height=height, source_file=source.name,
                  source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                  bitmap_sha256=hashlib.sha256(bitmap).hexdigest())
    config_path.write_text(json.dumps(config, indent=2) + '\n')
    print(f'Prepared {width}x{height} title bitmap: {len(bitmap)} bytes')


if __name__ == '__main__':
    main()
