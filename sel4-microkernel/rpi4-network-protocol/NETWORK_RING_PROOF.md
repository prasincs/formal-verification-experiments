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

## Specification and known-attack grounding

The ring discipline proved here is not a novel protocol. It is the standard
free-running-counter SPSC ring used by the established shared-memory I/O
specifications, and each proved property forecloses an attack class with a
documented real-world instance:

- **Free-running counters, modulo power-of-two ring.** The same discipline as
  VirtIO split virtqueues (OASIS VirtIO 1.2, §2.7 "Split Virtqueues": `idx`
  "always increments" and wraps naturally; the ring index is `idx % queue_size`)
  and Xen's `io/ring.h` producer/consumer counters. `verified_slot` proves the
  index computed from any counter value stays below `RING_SIZE`; because 64
  divides 2^32, slot progression also stays continuous across u32 wraparound.
  seL4's own device driver framework (sDDF, seL4 RFC-12) mandates exactly this
  shape: lock-free, bounded, single-producer/single-consumer queues in shared
  memory.

- **Peer-controlled ring state is untrusted input.** Each side's read of the
  other side's counter comes from writable shared memory. A compromised peer
  writing an out-of-range or inconsistent counter is the attack class behind
  CVE-2019-14835 (vhost-net: guest-supplied descriptor state with invalid
  length overflows a host kernel buffer). Here, `occupancy` uses
  `wrapping_sub`, so a corrupted read counter (`read > write`) makes occupancy
  appear ≥ 64 and `producer_permit` fails closed with `RingFull` instead of
  overwriting in-flight entries; slot selection is proved in-bounds for *any*
  counter value, so no peer-written counter can index outside the ring.

- **Ownership transfer, not rechecking.** XSA-155 (CVE-2015-8550) showed that
  Xen PV backends re-fetching request fields from the shared ring after
  validation let a guest flip them between check and use (a double fetch /
  TOCTOU on ring memory). The `VALID`-bit handoff proved here is the
  discipline that forecloses that class: an entry belongs to exactly one side
  at a time, the producer may not touch a published (consumer-owned) entry
  (`EntryNotReleased`), and the consumer may not read an unpublished one
  (`EntryNotPublished`). The network PD additionally reads `flags` and
  `length` once via `read_volatile` and clamps `length` to the entry buffer
  (`len.min(entry.data.len())`) — the same read-once-and-bound rule the
  XSA-155 fix (`RING_COPY_REQUEST`) adopted.

Residual risks that this proof deliberately does **not** cover, with their
spec anchors:

- **Weak-memory publication order.** VirtIO 1.2 requires "a suitable memory
  barrier before the idx update" so the peer never observes the index advance
  before the entry contents. Both PDs now issue a `SeqCst` fence between
  writing entry contents and setting `VALID`, and between the flag update and
  the index publication — but this discipline lives in the PD code, not the
  proof. The Verus contract models sequential ownership only and says nothing
  about ordering.

- **Payload length trust.** The `length` clamp in the network PD is executable
  defense (the CVE-2019-14835-class mitigation) but is not itself covered by
  the Verus contract, which proves index/ownership state, not payload bounds.
