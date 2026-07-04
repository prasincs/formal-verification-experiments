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
    pub ends: [ChannelEnd; 2],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorityGraph {
    pub regions: BTreeMap<String, MemoryRegion>,
    pub pds: BTreeMap<String, ProtectionDomain>,
    pub channels: Vec<Channel>,
    /// Directed protected-procedure authority: caller -> callee.
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
    pub mapping_perms: Vec<MappingPerms>,
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
            mapping_perms: Vec::new(),
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
pub struct MappingPerms {
    pub pd: String,
    pub region: String,
    pub perms: String,
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
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Xml(error) => write!(f, "XML parse error: {error}"),
            Self::Toml(error) => write!(f, "property parse error: {error}"),
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
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<roxmltree::Error> for CheckError {
    fn from(error: roxmltree::Error) -> Self {
        Self::Xml(error)
    }
}

impl From<toml::de::Error> for CheckError {
    fn from(error: toml::de::Error) -> Self {
        Self::Toml(error)
    }
}

pub fn parse_system(xml: &str) -> Result<AuthorityGraph, CheckError> {
    let document = Document::parse(xml)?;
    let system = document.root_element();
    if !system.has_tag_name("system") {
        return Err(CheckError::Malformed("root element must be <system>".into()));
    }

    let mut graph = AuthorityGraph::default();

    for node in system
        .children()
        .filter(|node| node.has_tag_name("memory_region"))
    {
        let name = required_attr(node, "name")?;
        if graph.regions.contains_key(&name) {
            return Err(CheckError::Malformed(format!(
                "duplicate memory region {name}"
            )));
        }
        graph.regions.insert(
            name.clone(),
            MemoryRegion {
                name,
                phys_addr: node.attribute("phys_addr").map(ToOwned::to_owned),
            },
        );
    }

    for node in system
        .children()
        .filter(|node| node.has_tag_name("protection_domain"))
    {
        parse_pd(node, None, &mut graph)?;
    }

    for node in system.children().filter(|node| node.has_tag_name("channel")) {
        let mut ends = node
            .children()
            .filter(|child| child.has_tag_name("end"))
            .map(|child| {
                Ok(ChannelEnd {
                    pd: required_attr(child, "pd")?,
                    id: parse_u32(&required_attr(child, "id")?)?,
                    protected_procedure: parse_bool(child.attribute("pp")),
                })
            })
            .collect::<Result<Vec<_>, CheckError>>()?;
        if ends.len() != 2 {
            return Err(CheckError::Malformed(format!(
                "channel must have exactly two ends, found {}",
                ends.len()
            )));
        }
        let right = ends.pop().expect("length checked");
        let left = ends.pop().expect("length checked");
        graph.channels.push(Channel {
            ends: [left, right],
        });
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

    for channel in &graph.channels {
        let left = &channel.ends[0];
        let right = &channel.ends[1];
        for end in [left, right] {
            if !graph.pds.contains_key(&end.pd) {
                return Err(CheckError::Malformed(format!(
                    "channel references unknown PD {}",
                    end.pd
                )));
            }
        }
        // Microkit 2.1 semantics: pp="true" marks the caller end. The
        // directed authority therefore runs from that end to the opposite end.
        if left.protected_procedure {
            graph.pp_edges.insert((left.pd.clone(), right.pd.clone()));
        }
        if right.protected_procedure {
            graph.pp_edges.insert((right.pd.clone(), left.pd.clone()));
        }
    }

    Ok(graph)
}

fn parse_pd(
    node: Node<'_, '_>,
    parent: Option<String>,
    graph: &mut AuthorityGraph,
) -> Result<(), CheckError> {
    let name = required_attr(node, "name")?;
    if graph.pds.contains_key(&name) {
        return Err(CheckError::Malformed(format!("duplicate PD {name}")));
    }

    let mut mappings = Vec::new();
    let mut irqs = BTreeSet::new();
    for child in node.children().filter(|child| child.is_element()) {
        if child.has_tag_name("map") {
            mappings.push(Mapping {
                region: required_attr(child, "mr")?,
                perms: child.attribute("perms").unwrap_or("").to_owned(),
            });
        } else if child.has_tag_name("irq") {
            irqs.insert(parse_u32(&required_attr(child, "irq")?)?);
        }
    }

    graph.pds.insert(
        name.clone(),
        ProtectionDomain {
            name: name.clone(),
            parent,
            mappings,
            irqs,
        },
    );

    for child in node
        .children()
        .filter(|child| child.has_tag_name("protection_domain"))
    {
        parse_pd(child, Some(name.clone()), graph)?;
    }

    Ok(())
}

fn required_attr(node: Node<'_, '_>, name: &str) -> Result<String, CheckError> {
    node.attribute(name).map(ToOwned::to_owned).ok_or_else(|| {
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
    let result = if let Some(hex) = normalized.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
    } else {
        normalized.parse()
    };
    result.map_err(|_| CheckError::Malformed(format!("invalid integer {value}")))
}

pub fn parse_properties(input: &str) -> Result<Properties, CheckError> {
    let properties: Properties = toml::from_str(input)?;
    if properties.version != 1 {
        return Err(CheckError::Malformed(format!(
            "unsupported property language version {}",
            properties.version
        )));
    }
    Ok(properties)
}

pub fn validate(graph: &AuthorityGraph, properties: &Properties) -> Vec<Violation> {
    let mut violations = Vec::new();

    for rule in &properties.shared_only {
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

    for rule in &properties.exclusive {
        require_pd(graph, &rule.pd, "exclusive", &mut violations);
        require_region(graph, &rule.region, "exclusive", &mut violations);
        let expected = BTreeSet::from([rule.pd.clone()]);
        let actual = region_mappers(graph, &rule.region);
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

    for rule in &properties.no_device_mmio {
        let Some(pd) = graph.pds.get(&rule.pd) else {
            violations.push(Violation::new(
                "no_device_mmio",
                format!("unknown PD {}", rule.pd),
            ));
            continue;
        };
        let physical: Vec<_> = pd
            .mappings
            .iter()
            .filter(|mapping| is_physical_mapping(graph, mapping))
            .map(|mapping| mapping.region.clone())
            .collect();
        if !physical.is_empty() || !pd.irqs.is_empty() {
            violations.push(Violation::new(
                "no_device_mmio",
                format!(
                    "PD {} maps physical regions {:?} and owns IRQs {:?}",
                    rule.pd, physical, pd.irqs
                ),
            ));
        }
    }

    for rule in &properties.only_channels {
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

    for rule in &properties.no_pp_to {
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

    for rule in &properties.mapping_perms {
        require_pd(graph, &rule.pd, "mapping_perms", &mut violations);
        require_region(graph, &rule.region, "mapping_perms", &mut violations);
        if let Some(pd) = graph.pds.get(&rule.pd) {
            let matches: Vec<_> = pd
                .mappings
                .iter()
                .filter(|mapping| mapping.region == rule.region)
                .collect();
            if matches.len() != 1 {
                violations.push(Violation::new(
                    "mapping_perms",
                    format!(
                        "PD {} has {} mappings of region {}, expected exactly one",
                        rule.pd,
                        matches.len(),
                        rule.region
                    ),
                ));
            } else if matches[0].perms != rule.perms {
                violations.push(Violation::new(
                    "mapping_perms",
                    format!(
                        "PD {} maps region {} with perms {}, expected {}",
                        rule.pd, rule.region, matches[0].perms, rule.perms
                    ),
                ));
            }
        }
    }

    let declared_dma: BTreeSet<_> = properties
        .dma_capable
        .iter()
        .map(|rule| rule.pd.clone())
        .collect();
    for pd in &declared_dma {
        require_pd(graph, pd, "dma_capable", &mut violations);
    }
    for pd in graph.pds.values() {
        // Conservative over-approximation: every physical mapping is treated
        // as device/DMA authority. This cannot be bypassed by renaming a region.
        let physical_regions: Vec<_> = pd
            .mappings
            .iter()
            .filter(|mapping| is_physical_mapping(graph, mapping))
            .map(|mapping| mapping.region.clone())
            .collect();
        if !physical_regions.is_empty() && !declared_dma.contains(&pd.name) {
            violations.push(Violation::new(
                "dma_capable",
                format!(
                    "PD {} maps physical regions {:?} but is not declared device/DMA-capable",
                    pd.name, physical_regions
                ),
            ));
        }
    }

    for rule in &properties.restartable_ring {
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
                        "endpoint {endpoint} is neither {} nor its descendant",
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

fn is_physical_mapping(graph: &AuthorityGraph, mapping: &Mapping) -> bool {
    graph
        .regions
        .get(&mapping.region)
        .is_some_and(|region| region.phys_addr.is_some())
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
    let mut current = Some(pd.to_owned());
    let mut seen = BTreeSet::new();
    while let Some(name) = current {
        if name == ancestor {
            return true;
        }
        if !seen.insert(name.clone()) {
            return false;
        }
        current = graph.pds.get(&name).and_then(|domain| domain.parent.clone());
    }
    false
}

pub fn check_text(xml: &str, properties: &str) -> Result<AuthorityGraph, CheckError> {
    let graph = parse_system(xml)?;
    let properties = parse_properties(properties)?;
    let violations = validate(&graph, &properties);
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

pub fn check_file(system_path: &Path, properties_path: &Path) -> Result<AuthorityGraph, CheckError> {
    let xml = fs::read_to_string(system_path)?;
    let properties = fs::read_to_string(properties_path)?;
    check_text(&xml, &properties)
}
