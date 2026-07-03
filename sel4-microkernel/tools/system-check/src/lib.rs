use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use roxmltree::{Document, Node};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegion {
    pub name: String,
    pub phys_addr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub region: String,
    pub perms: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionDomain {
    pub name: String,
    pub parent: Option<String>,
    pub protected_procedure: bool,
    pub mappings: Vec<Mapping>,
    pub irqs: BTreeSet<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEnd {
    pub pd: String,
    pub id: u32,
    pub protected_procedure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub ends: Vec<ChannelEnd>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorityGraph {
    pub regions: BTreeMap<String, MemoryRegion>,
    pub pds: BTreeMap<String, ProtectionDomain>,
    pub channels: Vec<Channel>,
    /// Directed caller -> callee protected-procedure authority.
    pub pp_edges: BTreeSet<(String, String)>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Properties {
    pub version: u32,
    pub shared_only: Vec<SharedOnly>,
    pub exclusive: Vec<Exclusive>,
    pub no_device_mmio: Vec<NoDeviceMmio>,
    pub only_channels: Vec<OnlyChannels>,
    pub no_pp_to: Vec<NoPpTo>,
    pub dma_capable: Vec<DmaCapable>,
    pub restartable_ring: Vec<RestartableRing>,
}

impl Default for Properties {
    fn default() -> Self {
        Self {
            version: 1,
            shared_only: Vec::new(),
            exclusive: Vec::new(),
            no_device_mmio: Vec::new(),
            only_channels: Vec::new(),
            no_pp_to: Vec::new(),
            dma_capable: Vec::new(),
            restartable_ring: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedOnly {
    pub pds: Vec<String>,
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusive {
    pub region: String,
    pub pd: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoDeviceMmio {
    pub pd: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnlyChannels {
    pub pd: String,
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoPpTo {
    pub pd: String,
    pub target: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DmaCapable {
    pub pd: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartableRing {
    pub region: String,
    pub lifecycle_pd: String,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub property: &'static str,
    pub detail: String,
}

impl Violation {
    fn new(property: &'static str, detail: impl Into<String>) -> Self {
        Self {
            property,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.property, self.detail)
    }
}

#[derive(Debug)]
pub enum CheckError {
    Io(std::io::Error),
    Xml(roxmltree::Error),
    Toml(toml::de::Error),
    Malformed(String),
    Violations(Vec<Violation>),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Xml(err) => write!(f, "XML parse error: {err}"),
            Self::Toml(err) => write!(f, "property parse error: {err}"),
            Self::Malformed(detail) => write!(f, "malformed .system file: {detail}"),
            Self::Violations(violations) => {
                writeln!(f, "{} authority violation(s)", violations.len())?;
                for violation in violations {
                    writeln!(f, "  - {violation}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for CheckError {}

impl From<std::io::Error> for CheckError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<roxmltree::Error> for CheckError {
    fn from(value: roxmltree::Error) -> Self {
        Self::Xml(value)
    }
}

impl From<toml::de::Error> for CheckError {
    fn from(value: toml::de::Error) -> Self {
        Self::Toml(value)
    }
}

pub fn parse_system(xml: &str) -> Result<AuthorityGraph, CheckError> {
    let doc = Document::parse(xml)?;
    let system = doc
        .descendants()
        .find(|node| node.has_tag_name("system"))
        .ok_or_else(|| CheckError::Malformed("missing <system> root".into()))?;

    let mut graph = AuthorityGraph::default();

    for region in system
        .children()
        .filter(|node| node.has_tag_name("memory_region"))
    {
        let name = required_attr(region, "name")?.to_owned();
        if graph.regions.contains_key(&name) {
            return Err(CheckError::Malformed(format!(
                "duplicate memory region {name}"
            )));
        }
        graph.regions.insert(
            name.clone(),
            MemoryRegion {
                name,
                phys_addr: region.attribute("phys_addr").map(ToOwned::to_owned),
            },
        );
    }

    for pd in system
        .children()
        .filter(|node| node.has_tag_name("protection_domain"))
    {
        parse_pd(pd, None, &mut graph)?;
    }

    for channel_node in system
        .children()
        .filter(|node| node.has_tag_name("channel"))
    {
        let mut ends = Vec::new();
        for end in channel_node
            .children()
            .filter(|node| node.has_tag_name("end"))
        {
            let pd = required_attr(end, "pd")?.to_owned();
            let id = parse_u32(required_attr(end, "id")?)?;
            let protected_procedure = parse_bool(end.attribute("pp"));
            ends.push(ChannelEnd {
                pd,
                id,
                protected_procedure,
            });
        }
        if ends.len() != 2 {
            return Err(CheckError::Malformed(format!(
                "channel must contain exactly two ends, found {}",
                ends.len()
            )));
        }
        graph.channels.push(Channel { ends });
    }

    for channel in &graph.channels {
        for end in &channel.ends {
            if !graph.pds.contains_key(&end.pd) {
                return Err(CheckError::Malformed(format!(
                    "channel references unknown PD {}",
                    end.pd
                )));
            }
        }
        let left = &channel.ends[0];
        let right = &channel.ends[1];
        if left.protected_procedure
            || graph
                .pds
                .get(&left.pd)
                .is_some_and(|pd| pd.protected_procedure)
        {
            graph.pp_edges.insert((right.pd.clone(), left.pd.clone()));
        }
        if right.protected_procedure
            || graph
                .pds
                .get(&right.pd)
                .is_some_and(|pd| pd.protected_procedure)
        {
            graph.pp_edges.insert((left.pd.clone(), right.pd.clone()));
        }
    }

    for pd in graph.pds.values() {
        for mapping in &pd.mappings {
            if !graph.regions.contains_key(&mapping.region) {
                return Err(CheckError::Malformed(format!(
                    "PD {} maps unknown region {}",
                    pd.name, mapping.region
                )));
            }
        }
    }

    Ok(graph)
}

fn parse_pd(
    node: Node<'_, '_>,
    parent: Option<&str>,
    graph: &mut AuthorityGraph,
) -> Result<(), CheckError> {
    let name = required_attr(node, "name")?.to_owned();
    if graph.pds.contains_key(&name) {
        return Err(CheckError::Malformed(format!("duplicate PD {name}")));
    }

    let mut mappings = Vec::new();
    let mut irqs = BTreeSet::new();
    for child in node.children().filter(Node::is_element) {
        if child.has_tag_name("map") {
            mappings.push(Mapping {
                region: required_attr(child, "mr")?.to_owned(),
                perms: child.attribute("perms").unwrap_or("").to_owned(),
            });
        } else if child.has_tag_name("irq") {
            irqs.insert(parse_u32(required_attr(child, "irq")?)?);
        }
    }

    graph.pds.insert(
        name.clone(),
        ProtectionDomain {
            name: name.clone(),
            parent: parent.map(ToOwned::to_owned),
            protected_procedure: parse_bool(node.attribute("pp")),
            mappings,
            irqs,
        },
    );

    for child in node
        .children()
        .filter(|child| child.has_tag_name("protection_domain"))
    {
        parse_pd(child, Some(&name), graph)?;
    }

    Ok(())
}

fn required_attr<'a>(node: Node<'a, 'a>, name: &str) -> Result<&'a str, CheckError> {
    node.attribute(name).ok_or_else(|| {
        CheckError::Malformed(format!(
            "<{}> is missing required attribute {name}",
            node.tag_name().name()
        ))
    })
}

fn parse_bool(value: Option<&str>) -> bool {
    matches!(value, Some("true" | "1" | "yes"))
}

fn parse_u32(value: &str) -> Result<u32, CheckError> {
    let normalized = value.replace('_', "");
    let parsed = if let Some(hex) = normalized.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
    } else {
        normalized.parse()
    };
    parsed.map_err(|_| CheckError::Malformed(format!("invalid integer {value}")))
}

pub fn parse_properties(toml_text: &str) -> Result<Properties, CheckError> {
    let props: Properties = toml::from_str(toml_text)?;
    if props.version != 1 {
        return Err(CheckError::Malformed(format!(
            "unsupported property language version {}",
            props.version
        )));
    }
    Ok(props)
}

pub fn validate(graph: &AuthorityGraph, props: &Properties) -> Vec<Violation> {
    let mut violations = Vec::new();

    for rule in &props.shared_only {
        let expected_pds: BTreeSet<_> = rule.pds.iter().cloned().collect();
        let expected_regions: BTreeSet<_> = rule.regions.iter().cloned().collect();
        for pd in &expected_pds {
            require_pd(graph, pd, "shared_only", &mut violations);
        }
        for region in &expected_regions {
            require_region(graph, region, "shared_only", &mut violations);
        }

        let mut common: Option<BTreeSet<String>> = None;
        for pd_name in &expected_pds {
            if let Some(pd) = graph.pds.get(pd_name) {
                let mapped: BTreeSet<_> = pd
                    .mappings
                    .iter()
                    .map(|mapping| mapping.region.clone())
                    .collect();
                common = Some(match common {
                    None => mapped,
                    Some(existing) => existing.intersection(&mapped).cloned().collect(),
                });
            }
        }
        let common = common.unwrap_or_default();
        if common != expected_regions {
            violations.push(Violation::new(
                "shared_only",
                format!(
                    "PDs {:?} share {:?}, expected exactly {:?}",
                    expected_pds, common, expected_regions
                ),
            ));
        }

        for region in &expected_regions {
            let actual = region_mappers(graph, region);
            if actual != expected_pds {
                violations.push(Violation::new(
                    "shared_only",
                    format!(
                        "region {region} is mapped by {:?}, expected exactly {:?}",
                        actual, expected_pds
                    ),
                ));
            }
        }
    }

    for rule in &props.exclusive {
        require_pd(graph, &rule.pd, "exclusive", &mut violations);
        require_region(graph, &rule.region, "exclusive", &mut violations);
        let actual = region_mappers(graph, &rule.region);
        let expected = BTreeSet::from([rule.pd.clone()]);
        if actual != expected {
            violations.push(Violation::new(
                "exclusive",
                format!(
                    "region {} is mapped by {:?}, expected only {}",
                    rule.region, actual, rule.pd
                ),
            ));
        }
    }

    for rule in &props.no_device_mmio {
        let Some(pd) = graph.pds.get(&rule.pd) else {
            violations.push(Violation::new(
                "no_device_mmio",
                format!("unknown PD {}", rule.pd),
            ));
            continue;
        };
        let device_regions: Vec<_> = pd
            .mappings
            .iter()
            .filter(|mapping| {
                graph
                    .regions
                    .get(&mapping.region)
                    .is_some_and(|region| region.phys_addr.is_some())
            })
            .map(|mapping| mapping.region.clone())
            .collect();
        if !device_regions.is_empty() || !pd.irqs.is_empty() {
            violations.push(Violation::new(
                "no_device_mmio",
                format!(
                    "PD {} maps device regions {:?} and owns IRQs {:?}",
                    rule.pd, device_regions, pd.irqs
                ),
            ));
        }
    }

    for rule in &props.only_channels {
        require_pd(graph, &rule.pd, "only_channels", &mut violations);
        let expected: BTreeSet<_> = rule.peers.iter().cloned().collect();
        let actual = channel_peers(graph, &rule.pd);
        if actual != expected {
            violations.push(Violation::new(
                "only_channels",
                format!(
                    "PD {} has peers {:?}, expected exactly {:?}",
                    rule.pd, actual, expected
                ),
            ));
        }
    }

    for rule in &props.no_pp_to {
        require_pd(graph, &rule.pd, "no_pp_to", &mut violations);
        require_pd(graph, &rule.target, "no_pp_to", &mut violations);
        if graph
            .pp_edges
            .contains(&(rule.pd.clone(), rule.target.clone()))
        {
            violations.push(Violation::new(
                "no_pp_to",
                format!(
                    "PD {} can invoke {} by protected procedure",
                    rule.pd, rule.target
                ),
            ));
        }
    }

    let declared_dma: BTreeSet<_> = props
        .dma_capable
        .iter()
        .map(|rule| rule.pd.clone())
        .collect();
    for pd in &declared_dma {
        require_pd(graph, pd, "dma_capable", &mut violations);
    }
    for pd in graph.pds.values() {
        let dma_regions: Vec<_> = pd
            .mappings
            .iter()
            .filter(|mapping| {
                graph
                    .regions
                    .get(&mapping.region)
                    .is_some_and(|region| region.phys_addr.is_some())
                    && dma_like_region(&mapping.region)
            })
            .map(|mapping| mapping.region.clone())
            .collect();
        if !dma_regions.is_empty() && !declared_dma.contains(&pd.name) {
            violations.push(Violation::new(
                "dma_capable",
                format!(
                    "PD {} maps DMA-capable regions {:?} but is not explicitly declared",
                    pd.name, dma_regions
                ),
            ));
        }
    }

    for rule in &props.restartable_ring {
        require_region(graph, &rule.region, "restartable_ring", &mut violations);
        require_pd(
            graph,
            &rule.lifecycle_pd,
            "restartable_ring",
            &mut violations,
        );
        let expected: BTreeSet<_> = rule.endpoints.iter().cloned().collect();
        let actual = region_mappers(graph, &rule.region);
        if actual != expected {
            violations.push(Violation::new(
                "restartable_ring",
                format!(
                    "region {} is mapped by {:?}, expected endpoints {:?}",
                    rule.region, actual, expected
                ),
            ));
        }
        for endpoint in &expected {
            require_pd(graph, endpoint, "restartable_ring", &mut violations);
            if graph.pds.contains_key(endpoint)
                && graph.pds.contains_key(&rule.lifecycle_pd)
                && !is_descendant_or_same(graph, endpoint, &rule.lifecycle_pd)
            {
                violations.push(Violation::new(
                    "restartable_ring",
                    format!(
                        "endpoint {endpoint} is not {} and is not its descendant",
                        rule.lifecycle_pd
                    ),
                ));
            }
        }
    }

    violations
}

fn require_pd(
    graph: &AuthorityGraph,
    pd: &str,
    property: &'static str,
    violations: &mut Vec<Violation>,
) {
    if !graph.pds.contains_key(pd) {
        violations.push(Violation::new(property, format!("unknown PD {pd}")));
    }
}

fn require_region(
    graph: &AuthorityGraph,
    region: &str,
    property: &'static str,
    violations: &mut Vec<Violation>,
) {
    if !graph.regions.contains_key(region) {
        violations.push(Violation::new(
            property,
            format!("unknown region {region}"),
        ));
    }
}

fn region_mappers(graph: &AuthorityGraph, region: &str) -> BTreeSet<String> {
    graph
        .pds
        .values()
        .filter(|pd| pd.mappings.iter().any(|mapping| mapping.region == region))
        .map(|pd| pd.name.clone())
        .collect()
}

fn channel_peers(graph: &AuthorityGraph, pd: &str) -> BTreeSet<String> {
    let mut peers = BTreeSet::new();
    for channel in &graph.channels {
        if channel.ends[0].pd == pd {
            peers.insert(channel.ends[1].pd.clone());
        } else if channel.ends[1].pd == pd {
            peers.insert(channel.ends[0].pd.clone());
        }
    }
    peers
}

fn is_descendant_or_same(graph: &AuthorityGraph, pd: &str, ancestor: &str) -> bool {
    let mut current = Some(pd);
    let mut seen = BTreeSet::new();
    while let Some(name) = current {
        if name == ancestor {
            return true;
        }
        if !seen.insert(name.to_owned()) {
            return false;
        }
        current = graph.pds.get(name).and_then(|domain| domain.parent.as_deref());
    }
    false
}

fn dma_like_region(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["dma", "virtio", "genet", "sdio", "usb", "ethernet", "pcie"]
        .iter()
        .any(|needle| lower.contains(needle))
}

pub fn check_text(xml: &str, toml_text: &str) -> Result<AuthorityGraph, CheckError> {
    let graph = parse_system(xml)?;
    let props = parse_properties(toml_text)?;
    let violations = validate(&graph, &props);
    if violations.is_empty() {
        Ok(graph)
    } else {
        Err(CheckError::Violations(violations))
    }
}

pub fn property_path(system_path: &Path) -> PathBuf {
    let mut value = system_path.as_os_str().to_owned();
    value.push(".props.toml");
    PathBuf::from(value)
}

pub fn check_file(system_path: &Path, props_path: &Path) -> Result<AuthorityGraph, CheckError> {
    let xml = fs::read_to_string(system_path)?;
    let props = fs::read_to_string(props_path)?;
    check_text(&xml, &props)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const BASE: &str = r#"
<system>
  <memory_region name="work_ring" size="0x1000" />
  <memory_region name="device" size="0x1000" phys_addr="0x1000" />
  <memory_region name="net_dma" size="0x1000" phys_addr="0x2000" />
  <protection_domain name="supervisor" priority="200">
    <map mr="work_ring" vaddr="0x5000" perms="rw" />
    <protection_domain name="worker" priority="100">
      <map mr="work_ring" vaddr="0x5000" perms="rw" />
    </protection_domain>
  </protection_domain>
  <protection_domain name="policy" priority="150" pp="true">
    <map mr="device" vaddr="0x6000" perms="rw" />
    <irq irq="33" id="1" />
  </protection_domain>
  <protection_domain name="network" priority="140">
    <map mr="net_dma" vaddr="0x7000" perms="rw" />
  </protection_domain>
  <channel><end pd="worker" id="1"/><end pd="policy" id="1"/></channel>
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
    }

    #[test]
    fn widened_mapping_breaks_shared_only_fixture() {
        let widened = BASE.replace(
            "<map mr=\"work_ring\" vaddr=\"0x5000\" perms=\"rw\" />\n    <protection_domain",
            "<map mr=\"work_ring\" vaddr=\"0x5000\" perms=\"rw\" />\n    <map mr=\"device\" vaddr=\"0x6000\" perms=\"r\" />\n    <protection_domain",
        )
        .replace(
            "<map mr=\"work_ring\" vaddr=\"0x5000\" perms=\"rw\" />\n    </protection_domain>",
            "<map mr=\"work_ring\" vaddr=\"0x5000\" perms=\"rw\" />\n      <map mr=\"device\" vaddr=\"0x6000\" perms=\"r\" />\n    </protection_domain>",
        );
        let err = check_text(&widened, VALID_PROPS).unwrap_err();
        assert!(err.to_string().contains("shared_only"));
    }

    #[test]
    fn added_channel_breaks_only_channels_fixture() {
        let xml = BASE.replace(
            "</system>",
            "<channel><end pd=\"worker\" id=\"2\"/><end pd=\"network\" id=\"2\"/></channel></system>",
        );
        let err = check_text(&xml, VALID_PROPS).unwrap_err();
        assert!(err.to_string().contains("only_channels"));
    }

    #[test]
    fn device_or_irq_breaks_no_device_mmio_fixture() {
        let props = format!("{VALID_PROPS}\n[[no_device_mmio]]\npd = \"policy\"\n");
        let err = check_text(BASE, &props).unwrap_err();
        assert!(err.to_string().contains("no_device_mmio"));
    }

    #[test]
    fn sibling_endpoint_breaks_restartable_ring_fixture() {
        let xml = BASE.replace(
            "<protection_domain name=\"worker\" priority=\"100\">",
            "</protection_domain><protection_domain name=\"worker\" priority=\"100\">",
        );
        let err = check_text(&xml, VALID_PROPS).unwrap_err();
        assert!(err.to_string().contains("restartable_ring"));
    }

    #[test]
    fn undeclared_dma_owner_is_rejected() {
        let props = VALID_PROPS.replace(
            "[[dma_capable]]\npd = \"network\"\n",
            "",
        );
        let err = check_text(BASE, &props).unwrap_err();
        assert!(err.to_string().contains("dma_capable"));
    }

    #[test]
    fn no_pp_to_checks_direction() {
        let props = format!(
            "{VALID_PROPS}\n[[no_pp_to]]\npd = \"worker\"\ntarget = \"policy\"\n"
        );
        let err = check_text(BASE, &props).unwrap_err();
        assert!(err.to_string().contains("protected procedure"));
    }

    #[test]
    fn property_path_appends_suffix() {
        assert_eq!(
            property_path(Path::new("demo.system")),
            PathBuf::from("demo.system.props.toml")
        );
    }
}
