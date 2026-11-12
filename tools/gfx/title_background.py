"""Add the supplied bitmap behind the title menu, preserving its controls and scripts.

Python standard library only. Input is the pinned, unpacked menu/05_000_title.gfx.
The image is a preprocessed DefineBitsLossless2 ARGB payload; no game textures are vendored.
SWF records follow Adobe SWF specification v19; GFx keeps the original GFX signature/version.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib


class Bits:
    def __init__(self):
        self.values = []

    def put(self, value, count):
        value &= (1 << count) - 1
        self.values.extend((value >> bit) & 1 for bit in range(count - 1, -1, -1))

    def bytes(self):
        values = self.values + [0] * (-len(self.values) % 8)
        return bytes(sum(values[i + bit] << (7 - bit) for bit in range(8)) for i in range(0, len(values), 8))


def rect(width, height):
    bits = Bits()
    count = max(width, height).bit_length() + 1
    bits.put(count, 5)
    for value in (0, width, 0, height):
        bits.put(value, count)
    return bits.bytes()


def bitmap_matrix(width, height, stage_width, stage_height):
    bits = Bits()
    scales = [round(stage_width * 20 / width * 65536), round(stage_height * 20 / height * 65536)]
    count = max(scales).bit_length() + 1
    if count > 31:
        raise ValueError("Bitmap scale does not fit a SWF matrix")
    bits.put(1, 1)  # HasScale
    bits.put(count, 5)
    for value in scales:
        bits.put(value, count)
    bits.put(0, 1)  # HasRotate
    bits.put(0, 5)  # NTranslateBits, zero translation
    return bits.bytes()


def shape_records(width, height):
    bits = Bits()
    bits.put(1, 4)  # NumFillBits
    bits.put(0, 4)  # NumLineBits
    bits.put(0, 1)  # StyleChangeRecord
    bits.put(0b00101, 5)  # FillStyle1 + MoveTo
    bits.put(1, 5)
    bits.put(0, 1)
    bits.put(0, 1)
    bits.put(1, 1)  # FillStyle1 index
    for vertical, delta in [(False, width), (True, height), (False, -width), (True, -height)]:
        count = abs(delta).bit_length() + 1
        if count > 17:
            raise ValueError("Stage edge does not fit a SWF straight edge")
        bits.put(1, 1)
        bits.put(1, 1)
        bits.put(count - 2, 4)
        bits.put(0, 1)  # GeneralLineFlag
        bits.put(vertical, 1)
        bits.put(delta, count)
    bits.put(0, 6)  # EndShapeRecord
    return bits.bytes()


def tag(kind, payload):
    if len(payload) < 63:
        return struct.pack('<H', kind << 6 | len(payload)) + payload
    return struct.pack('<HI', kind << 6 | 63, len(payload)) + payload


def tags(movie):
    if movie[:3] != b'GFX' or struct.unpack_from('<I', movie, 4)[0] != len(movie):
        raise ValueError("Expected an uncompressed, intact GFx movie")
    rect_bits = movie[8] >> 3
    cursor = 8 + (5 + 4 * rect_bits + 7) // 8 + 4
    while cursor < len(movie):
        start = cursor
        header = struct.unpack_from('<H', movie, cursor)[0]
        cursor += 2
        size = header & 63
        if size == 63:
            size = struct.unpack_from('<I', movie, cursor)[0]
            cursor += 4
        end = cursor + size
        if end > len(movie):
            raise ValueError("Truncated GFx tag")
        yield start, header >> 6, movie[cursor:end]
        cursor = end
    if cursor != len(movie):
        raise ValueError("Invalid GFx length")


def patch(movie, config, bitmap):
    if hashlib.sha256(movie).hexdigest() != config['game_file_sha256']:
        raise ValueError("The title movie differs from pinned game 1.03.3; verify it before patching")
    if hashlib.sha256(bitmap).hexdigest() != config['bitmap_sha256']:
        raise ValueError("Background bitmap checksum differs")
    width, height = config['width'], config['height']
    if len(zlib.decompress(bitmap)) != width * height * 4:
        raise ValueError("Incorrect ARGB bitmap size")
    bitmap_id, shape_id = 65000, 65001
    stage_width, stage_height = config['stage_width'], config['stage_height']
    bitmap_tag = tag(36, struct.pack('<HBHH', bitmap_id, 5, width, height) + bitmap)
    shape = struct.pack('<H', shape_id) + rect(stage_width * 20, stage_height * 20)
    shape += b'\x01\x41' + struct.pack('<H', bitmap_id)  # One smoothed, clipped bitmap fill
    shape += bitmap_matrix(width, height, stage_width, stage_height)
    shape += b'\x00' + shape_records(stage_width * 20, stage_height * 20)  # No lines
    shape_tag = tag(32, shape)
    place = tag(26, struct.pack('<BHH', 0x26, 1, shape_id) + b'\x00NrpvpTitleBackground\x00')
    records = list(tags(movie))
    definitions = {struct.unpack_from('<H', payload)[0] for _, kind, payload in records if kind in (2, 22, 32, 36, 37, 39, 83, 1009)}
    if bitmap_id in definitions or shape_id in definitions:
        raise ValueError("Background character IDs collide")
    first = records[0][0]
    output = bytearray(movie[:first])
    inserted = False
    shifted = 0
    for _, kind, payload in records:
        if kind == 26:
            if payload[0] & 0x40:
                raise ValueError("Unexpected root mask: review its clip depth before patching")
            depth = struct.unpack_from('<H', payload, 1)[0]
            if not inserted:
                output += bitmap_tag + shape_tag + place
                inserted = True
            payload = payload[:1] + struct.pack('<H', depth + 1) + payload[3:]
            shifted += 1
        elif kind in (4, 5, 28, 70):
            raise ValueError("Unexpected root display-list operation; verify before patching")
        output += tag(kind, payload)
    if not inserted or shifted != 3:
        raise ValueError("The pinned title root layout changed")
    struct.pack_into('<I', output, 4, len(output))
    return bytes(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--assets', type=Path, default=Path(__file__).resolve().parents[2] / 'gamedata/title')
    args = parser.parse_args()
    config = json.loads((args.assets / 'background.json').read_text())
    source_image = args.assets / config['source_file']
    if hashlib.sha256(source_image.read_bytes()).hexdigest() != config['source_sha256']:
        raise ValueError("The source image changed. Run tools/gfx/prepare_title_bitmap.py to regenerate its bitmap.")
    output = patch(args.source.read_bytes(), config, (args.assets / 'background.argb.zlib').read_bytes())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(output)
    print(f"Title background: {config['width']}x{config['height']} stretched to {config['stage_width']}x{config['stage_height']}: {args.output}")


if __name__ == '__main__':
    main()
