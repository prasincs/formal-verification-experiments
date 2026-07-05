use system_check::{check_text, parse_system};

const BASE: &str = r#"
<system>
  <memory_region name="work_ring" size="0x1000"/>
  <memory_region name="device" size="0x1000" phys_addr="0x1000"/>
  <memory_region name="pkt_buf" size="0x1000" phys_addr="0x2000"/>
  <protection_domain name="supervisor" priority="200">
    <map mr="work_ring" vaddr="0x5000" perms="rw"/>
    <protection_domain name="worker" priority="100">
      <map mr="work_ring" vaddr="0x5000" perms="rw"/>
    </protection_domain>
  </protection_domain>
  <protection_domain name="policy" priority="150">
    <map mr="device" vaddr="0x6000" perms="rw"/>
    <irq irq="33" id="1"/>
  </protection_domain>
  <protection_domain name="network" priority="140">
    <map mr="pkt_buf" vaddr="0x7000" perms="rw"/>
  </protection_domain>
  <channel><end pd="worker" id="1" pp="true"/><end pd="policy" id="1"/></channel>
</system>
"#;

const PROPS: &str = r#"
version = 1
[[shared_only]]
pds = ["supervisor", "worker"]
regions = ["work_ring"]
[[only_channels]]
pd = "worker"
peers = ["policy"]
[[mapping_perms]]
pd = "worker"
region = "work_ring"
perms = "rw"
[[dma_capable]]
pd = "policy"
[[dma_capable]]
pd = "network"
[[restartable_ring]]
region = "work_ring"
lifecycle_pd = "supervisor"
endpoints = ["supervisor", "worker"]
"#;

#[test]
fn accepts_complete_graph() {
    let graph = check_text(BASE, PROPS).unwrap();
    assert_eq!(graph.pds["worker"].parent.as_deref(), Some("supervisor"));
    assert!(graph
        .pp_edges
        .contains(&(String::from("worker"), String::from("policy"))));
}

#[test]
fn widened_mapping_is_rejected() {
    let xml = BASE.replace(
        "<map mr=\"pkt_buf\" vaddr=\"0x7000\" perms=\"rw\"/>",
        "<map mr=\"pkt_buf\" vaddr=\"0x7000\" perms=\"rw\"/><map mr=\"work_ring\" vaddr=\"0x8000\" perms=\"r\"/>",
    );
    assert!(check_text(&xml, PROPS)
        .unwrap_err()
        .to_string()
        .contains("shared_only"));
}

#[test]
fn widened_permission_is_rejected() {
    let xml = BASE.replace(
        "<map mr=\"work_ring\" vaddr=\"0x5000\" perms=\"rw\"/>\n    </protection_domain>",
        "<map mr=\"work_ring\" vaddr=\"0x5000\" perms=\"rwx\"/>\n    </protection_domain>",
    );
    assert!(check_text(&xml, PROPS)
        .unwrap_err()
        .to_string()
        .contains("mapping_perms"));
}

#[test]
fn added_channel_is_rejected() {
    let xml = BASE.replace(
        "</system>",
        "<channel><end pd=\"worker\" id=\"2\"/><end pd=\"network\" id=\"2\"/></channel></system>",
    );
    assert!(check_text(&xml, PROPS)
        .unwrap_err()
        .to_string()
        .contains("only_channels"));
}

#[test]
fn device_and_irq_are_rejected_for_no_device_pd() {
    let props = format!("{PROPS}\n[[no_device_mmio]]\npd = \"policy\"\n");
    assert!(check_text(BASE, &props)
        .unwrap_err()
        .to_string()
        .contains("no_device_mmio"));
}

#[test]
fn caller_end_pp_direction_is_rejected() {
    let props = format!("{PROPS}\n[[no_pp_to]]\npd = \"worker\"\ntarget = \"policy\"\n");
    assert!(check_text(BASE, &props)
        .unwrap_err()
        .to_string()
        .contains("no_pp_to"));
}

#[test]
fn unrelated_lifecycle_pd_is_rejected() {
    let props = PROPS.replace("lifecycle_pd = \"supervisor\"", "lifecycle_pd = \"policy\"");
    assert!(check_text(BASE, &props)
        .unwrap_err()
        .to_string()
        .contains("restartable_ring"));
}

#[test]
fn undeclared_physical_owner_is_rejected_after_neutral_rename() {
    let props = PROPS.replace("[[dma_capable]]\npd = \"network\"\n", "");
    let error = check_text(BASE, &props).unwrap_err().to_string();
    assert!(error.contains("dma_capable"));
    assert!(error.contains("pkt_buf"));
}

#[test]
fn malformed_channel_is_rejected_during_parse() {
    let xml = BASE.replace(
        "<channel><end pd=\"worker\" id=\"1\" pp=\"true\"/><end pd=\"policy\" id=\"1\"/></channel>",
        "<channel><end pd=\"worker\" id=\"1\"/></channel>",
    );
    assert!(parse_system(&xml).is_err());
}
