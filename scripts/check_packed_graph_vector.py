#!/usr/bin/env python3
"""Independent fixed framing for Decision 0152; no Rust or physical-layout dependency."""
import hashlib
import struct


def main():
    profile = hashlib.sha256(b"USTE graph-packed-v1").digest()
    assert profile.hex() == "c52ef9c7cafb1ec5c46bff34e454d3dfa975023efa0a07a2e7c07a4773e3faf8"
    assert hashlib.sha256(b"USTE graph-ordered-state-v1").hexdigest() == (
        "567bd84cd5ee364002fc4a781c7ff04f81abaffae9c6f238fef9ec8aba33a4ba")
    scope = bytes([1]) * 16 + bytes([2]) * 16
    families = b"".join(
        bytes([family]) + struct.pack(">QQ", 0, 0)
        + hashlib.sha256(b"USTE-ORDERED-COMMITMENT-V1\0" + scope + profile
                         + bytes([family, 0])).digest()
        for family in range(1, 9))
    digest = hashlib.sha256(b"USTE-GRAPH-ORDERED-STATE-V1\0" + scope
                            + struct.pack(">Q", 7) + bytes([3]) * 32 + profile + families)
    assert digest.hexdigest() == "bc4a4f2e69c0fcf4226bdb0b883deb9ec6073a9bb35733e3823be771b2064abf"
    print("packed_graph_vector=ok profiles=2 states=1")


if __name__ == "__main__":
    main()
