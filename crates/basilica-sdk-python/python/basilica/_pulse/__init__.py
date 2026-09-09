"""PULSE wire-format codec, vendored for the publisher surface (#1666).

The NORMATIVE format lives in basilica-backend
(docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4-C2.5, implemented in
python/pulse-verl/src/pulse_verl); this vendored copy must track it
byte-for-byte — parity is pinned by tests/test_pulse_codec.py.

IMPORT CONTRACT: nothing here may be imported at `import basilica` time.
The submodules need torch / numpy / xxhash / zstandard / safetensors —
publisher-only dependencies the base SDK deliberately does not carry.
`basilica.publisher` imports them lazily inside the publish paths and
raises an actionable error naming the missing extras.
"""
