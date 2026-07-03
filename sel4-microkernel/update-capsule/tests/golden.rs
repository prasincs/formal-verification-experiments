use ed25519_dalek::VerifyingKey;
use update_capsule::dalek::DalekVerifier;
use update_capsule::{payload_type, verify_capsule, VerificationContext};

#[test]
fn committed_vector_verifies() {
    let capsule = hex::decode(include_str!("../test-vectors/pd-code-v2.hex").trim()).unwrap();
    let public_key = hex::decode("ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c")
        .unwrap();
    let verifier = DalekVerifier(VerifyingKey::from_bytes(&public_key.try_into().unwrap()).unwrap());
    let context = VerificationContext {
        target_platform: 1,
        target_slot: 3,
        payload_type: payload_type::PD_CODE,
        abi_version: 4,
        signer_key_id: 12,
        key_epoch: 2,
        slot_base: 0x5000_0000,
        trusted_unix_time: None,
        scoped_rollback_version: 8,
    };
    let verified = verify_capsule(&capsule, &context, &verifier).unwrap();
    assert_eq!(verified.payload, b"worker-v2");
}
