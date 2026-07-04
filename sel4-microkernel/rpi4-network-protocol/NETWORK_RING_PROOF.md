# Concrete TX/RX ring proof boundary

WP-11 proves the ownership discipline of the existing `NetSharedMemory` rings.
It does not introduce a replacement ring and does not use generation state.

The concrete fields are:

- TX: `tx_write_idx`, `tx_read_idx`, and `tx_ring[]`;
- RX: `rx_write_idx`, `rx_read_idx`, and `rx_ring[]`.

Both directions use the executable helpers in `src/proof.rs`:

- `slot_for(counter)` invokes the Verus-checked modulo implementation;
- `producer_permit` requires fewer than 64 outstanding entries and requires
  the selected entry's existing `VALID` bit to be clear;
- `consumer_permit` requires a nonempty ring and requires `VALID` to be set.

`src/ring_contract.rs` proves modulo index bounds, the one-producer/one-consumer
counter invariant, and entry ownership transfer. A producer cannot publish a
consumer-owned entry; the consumer must release it first.

`ring_flags::IN_USE` remains defined only for source/ABI compatibility and is
deprecated. It is not a lock. The write/read counters select the sole current
producer/consumer slot, while `VALID` transfers that entry's ownership.

The proof assumes one producer and one consumer per direction. It covers index
bounds and entry reuse, not packet parsing, arbitrary MPSC/MPMC access, or
notification liveness.
