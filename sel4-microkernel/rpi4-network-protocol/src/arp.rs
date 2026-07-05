//! Minimal ARP-over-Ethernet frame encoding (IPv4).
//!
//! One set of named field offsets shared by the builder and the parser, so
//! client PDs never hand-poke byte offsets into ring entries. This is the
//! seed for the roadmap's packet-parser verification item.

/// Length of an Ethernet ARP frame for IPv4 (14-byte Ethernet header plus
/// 28-byte ARP payload, before any padding).
pub const FRAME_LEN: usize = 42;

const ETH_DST: usize = 0;
const ETH_SRC: usize = 6;
const ETH_TYPE: usize = 12;
const ARP_HTYPE: usize = 14;
const ARP_PTYPE: usize = 16;
const ARP_HLEN: usize = 18;
const ARP_PLEN: usize = 19;
const ARP_OPER: usize = 20;
const ARP_SHA: usize = 22;
const ARP_SPA: usize = 28;
const ARP_THA: usize = 32;
const ARP_TPA: usize = 38;

const ETHERTYPE_ARP: [u8; 2] = [0x08, 0x06];
const HTYPE_ETHERNET: [u8; 2] = [0x00, 0x01];
const PTYPE_IPV4: [u8; 2] = [0x08, 0x00];
const OPER_REQUEST: [u8; 2] = [0x00, 0x01];
const OPER_REPLY: [u8; 2] = [0x00, 0x02];

/// Sender fields of a parsed ARP reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArpReply {
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
}

/// Build a broadcast ARP request from `sender` for `target_ip` into `buf`;
/// returns the frame length.
///
/// # Panics
/// Panics if `buf` is shorter than [`FRAME_LEN`].
pub fn build_request(
    buf: &mut [u8],
    sender_mac: &[u8; 6],
    sender_ip: &[u8; 4],
    target_ip: &[u8; 4],
) -> usize {
    buf[ETH_DST..ETH_DST + 6].fill(0xff);
    buf[ETH_SRC..ETH_SRC + 6].copy_from_slice(sender_mac);
    buf[ETH_TYPE..ETH_TYPE + 2].copy_from_slice(&ETHERTYPE_ARP);
    buf[ARP_HTYPE..ARP_HTYPE + 2].copy_from_slice(&HTYPE_ETHERNET);
    buf[ARP_PTYPE..ARP_PTYPE + 2].copy_from_slice(&PTYPE_IPV4);
    buf[ARP_HLEN] = 6;
    buf[ARP_PLEN] = 4;
    buf[ARP_OPER..ARP_OPER + 2].copy_from_slice(&OPER_REQUEST);
    buf[ARP_SHA..ARP_SHA + 6].copy_from_slice(sender_mac);
    buf[ARP_SPA..ARP_SPA + 4].copy_from_slice(sender_ip);
    buf[ARP_THA..ARP_THA + 6].fill(0x00);
    buf[ARP_TPA..ARP_TPA + 4].copy_from_slice(target_ip);
    FRAME_LEN
}

/// Parse `frame` as an IPv4-over-Ethernet ARP reply, returning the sender's
/// MAC and IP. Returns `None` for anything that is not a well-formed reply.
pub fn parse_reply(frame: &[u8]) -> Option<ArpReply> {
    if frame.len() < FRAME_LEN {
        return None;
    }
    if frame[ETH_TYPE..ETH_TYPE + 2] != ETHERTYPE_ARP
        || frame[ARP_HTYPE..ARP_HTYPE + 2] != HTYPE_ETHERNET
        || frame[ARP_PTYPE..ARP_PTYPE + 2] != PTYPE_IPV4
        || frame[ARP_HLEN] != 6
        || frame[ARP_PLEN] != 4
        || frame[ARP_OPER..ARP_OPER + 2] != OPER_REPLY
    {
        return None;
    }

    let mut sender_mac = [0u8; 6];
    sender_mac.copy_from_slice(&frame[ARP_SHA..ARP_SHA + 6]);
    let mut sender_ip = [0u8; 4];
    sender_ip.copy_from_slice(&frame[ARP_SPA..ARP_SPA + 4]);
    Some(ArpReply {
        sender_mac,
        sender_ip,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    const GW_MAC: [u8; 6] = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];
    const IP: [u8; 4] = [10, 0, 2, 15];
    const GW_IP: [u8; 4] = [10, 0, 2, 2];

    fn reply_frame() -> [u8; FRAME_LEN] {
        let mut frame = [0u8; FRAME_LEN];
        build_request(&mut frame, &GW_MAC, &GW_IP, &IP);
        frame[ARP_OPER..ARP_OPER + 2].copy_from_slice(&OPER_REPLY);
        frame
    }

    #[test]
    fn request_is_not_a_reply() {
        let mut frame = [0u8; FRAME_LEN];
        let len = build_request(&mut frame, &MAC, &IP, &GW_IP);
        assert_eq!(len, FRAME_LEN);
        assert_eq!(parse_reply(&frame), None);
    }

    #[test]
    fn reply_sender_fields_round_trip() {
        let reply = parse_reply(&reply_frame()).unwrap();
        assert_eq!(reply.sender_mac, GW_MAC);
        assert_eq!(reply.sender_ip, GW_IP);
    }

    #[test]
    fn short_or_non_arp_frames_are_rejected() {
        let frame = reply_frame();
        assert_eq!(parse_reply(&frame[..FRAME_LEN - 1]), None);

        let mut not_arp = frame;
        not_arp[ETH_TYPE] = 0x08;
        not_arp[ETH_TYPE + 1] = 0x00;
        assert_eq!(parse_reply(&not_arp), None);

        let mut bad_plen = frame;
        bad_plen[ARP_PLEN] = 16;
        assert_eq!(parse_reply(&bad_plen), None);
    }
}
