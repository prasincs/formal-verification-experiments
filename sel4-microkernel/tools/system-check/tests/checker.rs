use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use system_check::{check_text, property_path};

const BASE: &str = r#"
<system>
  <memory_region name="work_ring" size="0x1000" />
  <memory_region name="device" size="0x1000" phys_addr="0x1000" />
  <memory_region name="pkt_buf" size="0x1000" phys_addr="0x2000" />
  <protection_domain name="supervisor" priority="200">
    <map mr="work_ring" vaddr="0x5000" perms="rw" />
    <protection_domain name="worker" priority="100">
      <map mr="work_ring" vaddr="0x5000" perms="rw" />
    </protection_domain>
  </protection_domain>
  <protection_domain name="policy" priority="150">
    <map mr="device" vaddr="0x6000" perms="rw" />
    <irq irq="33" id="1" />
  </protection_domain>
  <protection_domain name="network" priority="140">
    <map mr="pkt_buf" vaddr="0x7000" perms="rw" />
  </protection_domain>
  <channel><end pd="worker" id="1" pp="true"/><end pd="policy" id="1"/></channel>
</system>
"#;

const VALID_PROPS: &str = r#"
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
fn parses_complete_authority_graph() {
    let graph = check_text(BASE, VALID_PROPS).unwrap();
    assert_eq!(graph.pds["worker"].parent.as_deref(), Some("supervisor"));
    assert_eq!(graph.pds["policy"].irqs, BTreeSet::from([33]));
    assert!(graph
        .pp_edges
        .contains(&(String::from("worker"), String::from("policy"))));
    assert!(!graph
        .pp_edges
        .contains(&(String::from("policy"), String::from("worker"))));
}

#[test]
fn no_pp_to_uses_caller_end_direction() {
    let properties = format!(
        "{VALID_PROPS}\n[[no_pp_to]]\npd = \"worker\"\ntarget = \"policy\"\n"
    );
    let error = check_text(BASE, &properties).unwrap_err();
    assert!(error.to_string().contains("protected procedure"));
}

#[test]
fn reversed_no_pp_to_is_allowed() {
    let properties = format!(
        "{VALID_PROPS}\n[[no_pp_to]]\npd = \"policy\"\ntarget = \"worker\"\n"
    );
    check_text(BASE, &properties).unwrap();
}

#[test]
fn added_mapping_breaks_shared_only() {
    let widened = BASE.replace(
        "<map mr=\"pkt_buf\" vaddr=\"0x7000\" perms=\"rw\" />",
        "<map mr=\"pkt_buf\" vaddr=\"0x7000\" perms=\"rw\" /><map mr=\"work_ring\" vaddr=\"0x8000\" perms=\"r\" />",
    );
    let error = check_text(&widened, VALID_PROPS).unwrap_err();
    assert!(error.to_string().contains("shared_only"));
}

#[test]
fn widened_permissions_are_rejected() {
    let widened = BASE.replace(
        "<map mr=\"work_ring\" vaddr=\"0x5000\" perms=\"rw\" />\n    </protection_domain>",
        "<map mr=\"work_ring\" vaddr=\"0x5000\" perms=\"rwx\" />\n    </protection_domain>",
    );
    let error = check_text(&widened, VALID_PROPS).unwrap_err();
    assert!(error.to_string().contains("mapping_perms"));
}

#[test]
fn added_channel_breaks_only_channels() {
    let xml = BASE.replace(
        "</system>",
        "<channel><end pd=\"worker\" id=\"2\"/><end pd=\"network\" id=\"2\"/></channel></system>",
    );
    let error = check_text(&xml, VALID_PROPS).unwrap_err();
    assert!(error.to_string().contains("only_channels"));
}

#[test]
fn device_or_irq_breaks_no_device_mmio() {
    let properties = format!("{VALID_PROPS}\n[[no_device_mmio]]\npd = \"policy\"\n");
    let error = check_text(BASE, &properties).unwrap_err();
    assert!(error.to_string().contains("no_device_mmio"));
}

#[test]
fn sibling_endpoint_breaks_restartable_ring() {
    let xml = r#"
<system>
  <memory_region name="work_ring" size="0x1000" />
  <memory_region name="device" size="0x1000" phys_addr="0x1000" />
  <memory_region name="pkt_buf" size="0x1000" phys_addr="0x2000" />
  <protection_domain name="supervisor" priority="200">
    <map mr="work_ring" vaddr="0x5000" perms="rw" />
  </protection_domain>
  <protection_domain name="worker" priority="100">
    <map mr="work_ring" vaddr="0x5000" perms="rw" />
  </protection_domain>
  <protection_domain name="policy" priority="150">
    <map mr="device" vaddr="0x6000" perms="rw" />
    <irq irq="33" id="1" />
  </protection_domain>
  <protection_domain name="network" priority="140">
    <map mr="pkt_buf" vaddr="0x7000" perms="rw" />
  </protection_domain>
  <channel><end pd="worker" id="1" pp="true"/><end pd="policy" id="1"/></channel>
</system>
"#;
    let error = check_text(xml, VALID_PROPS).unwrap_err();
    assert!(error.to_string().contains("restartable_ring"));
}

#[test]
fn undeclared_physical_owner_is_rejected_without_name_heuristics() {
    let properties = VALID_PROPS.replace("[[dma_capable]]\npd = \"network\"\n", "");
    let error = check_text(BASE, &properties).unwrap_err();
    assert!(error.to_string().contains("dma_capable"));
    assert!(error.to_string().contains("pkt_buf"));
}

#[test]
fn property_path_appends_sidecar_suffix() {
    assert_eq!(
        property_path(Path::new("demo.system")),
        PathBuf::from("demo.system.props.toml")
    );
}
