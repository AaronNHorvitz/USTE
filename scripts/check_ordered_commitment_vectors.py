#!/usr/bin/env python3
"""Independent standard-library reconstruction of Decision 0128's synthetic vectors."""
import hashlib
import struct

PREFIX = (b"USTE-ORDERED-COMMITMENT-V1\0" + bytes([1]) * 16
          + bytes([2]) * 16 + bytes([3]) * 32 + bytes([7]))


def value_digest(value):
    return hashlib.sha256(b"USTE-ORDERED-COMMITMENT-VALUE-V1\0"
                          + struct.pack(">Q", len(value)) + value).digest()


def key_bits(key):
    result = []
    for byte in key:
        result.append(1)
        result.extend((byte >> shift) & 1 for shift in range(7, -1, -1))
    return result + [0]


def summary_frame(summary):
    count, size, digest = summary
    return struct.pack(">QQ", count, size) + digest


def reconstruct(items):
    if not items:
        return 0, 0, hashlib.sha256(PREFIX + b"\0").digest()
    if len(items) == 1:
        key, value = items[0]
        frame = (PREFIX + b"\1" + struct.pack(">I", len(key)) + key
                 + struct.pack(">Q", len(value)) + value_digest(value))
        return 1, len(key) + len(value), hashlib.sha256(frame).digest()
    first, last = key_bits(items[0][0]), key_bits(items[-1][0])
    first += [0] * max(0, len(last) - len(first))
    last += [0] * max(0, len(first) - len(last))
    branch = next(i for i, pair in enumerate(zip(first, last)) if pair[0] != pair[1])
    split = next(i for i, (key, _) in enumerate(items) if key_bits(key)[branch])
    assert 0 < split < len(items)
    left, right = reconstruct(items[:split]), reconstruct(items[split:])
    frame = PREFIX + b"\2" + struct.pack(">I", branch) + summary_frame(left) + summary_frame(right)
    return left[0] + right[0], left[1] + right[1], hashlib.sha256(frame).digest()


def main():
    vectors = [
        ([], 0, 0, "20e05dc475e75384475922992db42c0640e801f16de9b442b01a62ae5082373c"),
        ([(b"a", b"A")], 1, 2, "cf793bc0171193181684944a12a513ee923789274d402fcc319d1301f4168f8e"),
        ([(b"a", b"A"), (b"ab", b"BC"), (b"\0", b""), (b"\xff", b"z")], 4, 9,
         "0cd0830cfc9b147c5473d7e116e5e3dec6661fededa3763bdfedd4a0c9ffdc87"),
    ]
    for items, count, size, expected in vectors:
        actual = reconstruct(sorted(items))
        assert actual == (count, size, bytes.fromhex(expected))
    assert value_digest(b"A").hex() == "df25c5122e9028e790f47ff010b032be55738b6fbc9c4689735272368c0fb837"
    print("ordered_commitment_vectors=ok cases=4")


if __name__ == "__main__":
    main()
