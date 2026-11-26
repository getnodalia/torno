"""Checks shared by the desktop package smoke tests: control replies and the presented frame.

Used by snap-smoke.py: a package passes only when the app answered over the control channel,
modelled a box and drew a real, non-blank frame.
"""
import json
import math
import struct
import zlib

MAX_PIXELS = 16_000_000
MAX_PNG_BYTES = 70_000_000


def validate_replies(stdout):
    replies = [json.loads(line) for line in stdout.splitlines()]
    if len(replies) != 3 or any(reply.get("id") != i or reply.get("ok") is not True
                                for i, reply in enumerate(replies, 1)):
        raise ValueError(f"desktop control failed: {replies}")
    ui = replies[0]["result"]
    viewport = ui["viewport"]
    if (ui["renderer"] not in {"gpu", "cpu"} or len(viewport) != 4
            or not all(math.isfinite(value) for value in viewport)
            or viewport[2] <= 0 or viewport[3] <= 0):
        raise ValueError("desktop did not create a usable viewport")
    document = replies[2]["result"]
    bodies = document["bodies"]
    if (len(bodies) != 1 or len(document["timeline"]) != 1
            or document["timeline"][0]["error"] is not None
            or not math.isclose(bodies[0]["volume_mm3"], 6000, rel_tol=0, abs_tol=1e-7)
            or bodies[0]["bbox"] != {"min": [0, 0, 0], "max": [30, 20, 10]}):
        raise ValueError(f"desktop modelling failed: {document}")
    return ui["renderer"]


def frame_statistics(png):
    """Bounded PNG readback check for the app's non-interlaced RGBA8 output."""
    if len(png) > MAX_PNG_BYTES or not png.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("invalid or oversized presented-frame PNG")
    offset, width, height, ended = 8, 0, 0, False
    compressed = bytearray()
    while offset < len(png):
        if offset + 12 > len(png):
            raise ValueError("truncated PNG chunk")
        length = struct.unpack_from(">I", png, offset)[0]
        kind = png[offset + 4:offset + 8]
        end = offset + 12 + length
        if end > len(png):
            raise ValueError("truncated PNG data")
        data = memoryview(png)[offset + 8:offset + 8 + length]
        crc = zlib.crc32(data, zlib.crc32(kind)) & 0xffffffff
        if crc != struct.unpack_from(">I", png, end - 4)[0]:
            raise ValueError("PNG checksum mismatch")
        if kind == b"IHDR":
            if offset != 8 or length != 13:
                raise ValueError("invalid PNG header")
            width, height, depth, colour, compression, filtering, interlace = struct.unpack(">IIBBBBB", data)
            if (width == 0 or height == 0 or width * height > MAX_PIXELS
                    or (depth, colour, compression, filtering, interlace) != (8, 6, 0, 0, 0)):
                raise ValueError("unsupported PNG dimensions or format")
        elif kind == b"IDAT":
            if not width or len(compressed) + length > MAX_PNG_BYTES:
                raise ValueError("invalid PNG image data")
            compressed.extend(data)
        elif kind == b"IEND":
            if length or end != len(png):
                raise ValueError("invalid PNG end")
            ended = True
            break
        elif not (kind[0] & 32):
            raise ValueError("unsupported critical PNG chunk")
        offset = end
    if not ended or not width or not compressed:
        raise ValueError("incomplete PNG")
    stride = width * 4
    expected = height * (stride + 1)
    inflater = zlib.decompressobj()
    try:
        raw = inflater.decompress(compressed, expected + 1)
    except zlib.error as error:
        raise ValueError("invalid compressed PNG data") from error
    if (len(raw) != expected or not inflater.eof or inflater.unconsumed_tail or inflater.unused_data):
        raise ValueError("PNG decompressed size mismatch")
    previous = bytearray(stride)
    first = None
    nonblack, varied = 0, False
    for y in range(height):
        start = y * (stride + 1)
        method = raw[start]
        if method > 4:
            raise ValueError("unsupported PNG row filter")
        row = bytearray(raw[start + 1:start + 1 + stride])
        if method:
            for i in range(stride):
                left = row[i - 4] if i >= 4 else 0
                above = previous[i]
                corner = previous[i - 4] if i >= 4 else 0
                if method == 1:
                    prediction = left
                elif method == 2:
                    prediction = above
                elif method == 3:
                    prediction = (left + above) // 2
                else:
                    p = left + above - corner
                    a, b, c = abs(p - left), abs(p - above), abs(p - corner)
                    prediction = left if a <= b and a <= c else (above if b <= c else corner)
                row[i] = (row[i] + prediction) & 255
        if first is None:
            first = tuple(row[:3])
        for i in range(0, stride, 4):
            nonblack += row[i + 3] > 8 and (row[i] > 8 or row[i + 1] > 8 or row[i + 2] > 8)
            varied |= row[i] != first[0] or row[i + 1] != first[1] or row[i + 2] != first[2]
        previous = row
    if not varied or nonblack <= width * height // 10:
        raise ValueError(f"blank presented frame: {nonblack}/{width * height} nonblack pixels, varied={varied}")
    return {"width": width, "height": height, "nonblack": nonblack, "pixels": width * height}
