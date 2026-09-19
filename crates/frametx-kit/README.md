# kohaku-frametx-kit

8-field EIP-8141 FrameTx encoder for MSP HEAD / Hegota (chain 8141).

`0x06 || rlp([chain_id, nonce_keys, nonce_seq, sender, frames, signatures, fees, blob_hashes])`

Each frame is `rlp([mode, flags, target, [execution, state], value, data])`. Recent roots travel in `VERIFY(0x8272, 72-byte tuple)`, not an envelope field.
