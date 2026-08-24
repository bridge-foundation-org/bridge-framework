//! Redis Cluster Support - Clustering and high availability
//!
//! Manage Redis cluster topology, slot distribution, and failover

use std::collections::{HashMap, HashSet};

/// Redis cluster slot
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slot(u16);

impl Slot {
    /// Create slot from number (0-16383)
    pub fn new(num: u16) -> Result<Self, String> {
        if num < 16384 {
            Ok(Slot(num))
        } else {
            Err(format!("Slot {} out of range", num))
        }
    }

    /// Get slot number for key using CRC16
    pub fn for_key(key: &str) -> Slot {
        let hash_tag = extract_hash_tag(key);
        let crc = crc16_ccitt(hash_tag.as_bytes());
        Slot(crc % 16384)
    }

    /// Get numeric value
    pub fn num(&self) -> u16 {
        self.0
    }
}

/// Redis cluster node
#[derive(Clone, Debug)]
pub struct ClusterNode {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub is_master: bool,
    pub slots: HashSet<u16>,
}

impl ClusterNode {
    pub fn new(id: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        ClusterNode {
            id: id.into(),
            host: host.into(),
            port,
            is_master: true,
            slots: HashSet::new(),
        }
    }

    pub fn as_replica(mut self) -> Self {
        self.is_master = false;
        self
    }

    pub fn add_slot(&mut self, slot: u16) {
        self.slots.insert(slot);
    }

    pub fn remove_slot(&mut self, slot: u16) {
        self.slots.remove(&slot);
    }

    pub fn owns_slot(&self, slot: u16) -> bool {
        self.slots.contains(&slot)
    }

    pub fn get_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Cluster topology
#[derive(Clone, Debug)]
pub struct ClusterTopology {
    nodes: HashMap<String, ClusterNode>,
    slot_map: HashMap<u16, String>,         // slot -> node_id
    replicas: HashMap<String, Vec<String>>, // master_id -> replica_ids
}

impl ClusterTopology {
    pub fn new() -> Self {
        ClusterTopology {
            nodes: HashMap::new(),
            slot_map: HashMap::new(),
            replicas: HashMap::new(),
        }
    }

    /// Add node to cluster
    pub fn add_node(&mut self, node: ClusterNode) -> Result<(), String> {
        if self.nodes.contains_key(&node.id) {
            return Err(format!("Node {} already exists", node.id));
        }
        self.nodes.insert(node.id.clone(), node);
        Ok(())
    }

    /// Remove node from cluster
    pub fn remove_node(&mut self, node_id: &str) -> Result<(), String> {
        if !self.nodes.contains_key(node_id) {
            return Err(format!("Node {} not found", node_id));
        }

        // Remove slot mappings
        let slots_to_remove: Vec<_> = self
            .slot_map
            .iter()
            .filter(|(_, id)| *id == node_id)
            .map(|(&slot, _)| slot)
            .collect();

        for slot in slots_to_remove {
            self.slot_map.remove(&slot);
        }

        self.nodes.remove(node_id);
        self.replicas.remove(node_id);
        Ok(())
    }

    /// Get node by ID
    pub fn get_node(&self, node_id: &str) -> Option<&ClusterNode> {
        self.nodes.get(node_id)
    }

    /// List all nodes
    pub fn list_nodes(&self) -> Vec<&ClusterNode> {
        self.nodes.values().collect()
    }

    /// Assign slots to node
    pub fn assign_slots(&mut self, node_id: &str, slots: Vec<u16>) -> Result<(), String> {
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| format!("Node {} not found", node_id))?;

        for slot in slots {
            node.add_slot(slot);
            self.slot_map.insert(slot, node_id.to_string());
        }

        Ok(())
    }

    /// Get node for slot
    pub fn get_node_for_slot(&self, slot: u16) -> Option<&ClusterNode> {
        self.slot_map
            .get(&slot)
            .and_then(|node_id| self.nodes.get(node_id))
    }

    /// Add replica
    pub fn add_replica(&mut self, master_id: &str, replica_id: &str) -> Result<(), String> {
        if !self.nodes.contains_key(master_id) {
            return Err(format!("Master node {} not found", master_id));
        }
        if !self.nodes.contains_key(replica_id) {
            return Err(format!("Replica node {} not found", replica_id));
        }

        self.replicas
            .entry(master_id.to_string())
            .or_insert_with(Vec::new)
            .push(replica_id.to_string());

        Ok(())
    }

    /// Get replicas for master
    pub fn get_replicas(&self, master_id: &str) -> Vec<&ClusterNode> {
        self.replicas
            .get(master_id)
            .map(|replica_ids| {
                replica_ids
                    .iter()
                    .filter_map(|id| self.nodes.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Default for ClusterTopology {
    fn default() -> Self {
        Self::new()
    }
}

/// Cluster state tracker
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClusterState {
    Ok,
    Partial,
    Failed,
}

impl ClusterState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClusterState::Ok => "ok",
            ClusterState::Partial => "partial",
            ClusterState::Failed => "failed",
        }
    }
}

/// Cluster health info
#[derive(Clone, Debug)]
pub struct ClusterInfo {
    pub state: ClusterState,
    pub node_count: usize,
    pub slots_assigned: usize,
    pub slots_ok: usize,
    pub slots_fail: usize,
}

impl ClusterInfo {
    pub fn new(topology: &ClusterTopology) -> Self {
        let node_count = topology.nodes.len();
        let slots_assigned = topology.slot_map.len();
        let slots_ok = if slots_assigned == 16384 {
            16384
        } else {
            slots_assigned
        };

        let state = if node_count == 0 {
            ClusterState::Failed
        } else if slots_assigned < 16384 {
            ClusterState::Partial
        } else {
            ClusterState::Ok
        };

        ClusterInfo {
            state,
            node_count,
            slots_assigned,
            slots_ok,
            slots_fail: 16384 - slots_ok,
        }
    }
}

// CRC16-CCITT implementation for slot calculation
fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u32 = 0;
    for byte in data {
        crc ^= (*byte as u32) << 8;
        for _ in 0..8 {
            crc <<= 1;
            if crc & 0x10000 != 0 {
                crc ^= 0x1021;
            }
        }
    }
    (crc & 0xffff) as u16
}

// Extract hash tag from key (text between {})
fn extract_hash_tag(key: &str) -> String {
    if let Some(start) = key.find('{') {
        if let Some(end) = key.find('}') {
            if start < end {
                return key[start + 1..end].to_string();
            }
        }
    }
    key.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_new_valid() {
        let slot = Slot::new(1000);
        assert!(slot.is_ok());
        assert_eq!(slot.unwrap().num(), 1000);
    }

    #[test]
    fn test_slot_new_invalid() {
        let slot = Slot::new(16384);
        assert!(slot.is_err());
    }

    #[test]
    fn test_slot_for_key() {
        let slot = Slot::for_key("key123");
        assert!(slot.num() < 16384);
    }

    #[test]
    fn test_slot_for_key_with_hash_tag() {
        let slot1 = Slot::for_key("{user:1}:profile");
        let slot2 = Slot::for_key("{user:1}:settings");
        assert_eq!(slot1, slot2); // Same hash tag = same slot
    }

    #[test]
    fn test_cluster_node_new() {
        let node = ClusterNode::new("node1", "localhost", 6379);
        assert_eq!(node.id, "node1");
        assert!(node.is_master);
    }

    #[test]
    fn test_cluster_node_as_replica() {
        let node = ClusterNode::new("node1", "localhost", 6379).as_replica();
        assert!(!node.is_master);
    }

    #[test]
    fn test_cluster_node_slots() {
        let mut node = ClusterNode::new("node1", "localhost", 6379);
        node.add_slot(100);
        node.add_slot(200);
        assert!(node.owns_slot(100));
        assert!(node.owns_slot(200));
        assert!(!node.owns_slot(300));
    }

    #[test]
    fn test_cluster_topology_new() {
        let topology = ClusterTopology::new();
        assert_eq!(topology.list_nodes().len(), 0);
    }

    #[test]
    fn test_cluster_topology_add_node() {
        let mut topology = ClusterTopology::new();
        let node = ClusterNode::new("node1", "localhost", 6379);
        let result = topology.add_node(node);
        assert!(result.is_ok());
        assert_eq!(topology.list_nodes().len(), 1);
    }

    #[test]
    fn test_cluster_topology_add_duplicate_node() {
        let mut topology = ClusterTopology::new();
        let node1 = ClusterNode::new("node1", "localhost", 6379);
        let node2 = ClusterNode::new("node1", "localhost", 6380);

        topology.add_node(node1).unwrap();
        let result = topology.add_node(node2);
        assert!(result.is_err());
    }

    #[test]
    fn test_cluster_topology_remove_node() {
        let mut topology = ClusterTopology::new();
        let node = ClusterNode::new("node1", "localhost", 6379);
        topology.add_node(node).unwrap();

        let result = topology.remove_node("node1");
        assert!(result.is_ok());
        assert_eq!(topology.list_nodes().len(), 0);
    }

    #[test]
    fn test_cluster_topology_get_node() {
        let mut topology = ClusterTopology::new();
        let node = ClusterNode::new("node1", "localhost", 6379);
        topology.add_node(node).unwrap();

        let retrieved = topology.get_node("node1");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_cluster_topology_assign_slots() {
        let mut topology = ClusterTopology::new();
        let node = ClusterNode::new("node1", "localhost", 6379);
        topology.add_node(node).unwrap();

        let result = topology.assign_slots("node1", vec![0, 100, 200]);
        assert!(result.is_ok());

        let retrieved = topology.get_node("node1").unwrap();
        assert!(retrieved.owns_slot(100));
    }

    #[test]
    fn test_cluster_topology_get_node_for_slot() {
        let mut topology = ClusterTopology::new();
        let node = ClusterNode::new("node1", "localhost", 6379);
        topology.add_node(node).unwrap();
        topology.assign_slots("node1", vec![100]).unwrap();

        let node_for_slot = topology.get_node_for_slot(100);
        assert!(node_for_slot.is_some());
        assert_eq!(node_for_slot.unwrap().id, "node1");
    }

    #[test]
    fn test_cluster_topology_add_replica() {
        let mut topology = ClusterTopology::new();
        let master = ClusterNode::new("master", "localhost", 6379);
        let replica = ClusterNode::new("replica", "localhost", 6380).as_replica();

        topology.add_node(master).unwrap();
        topology.add_node(replica).unwrap();

        let result = topology.add_replica("master", "replica");
        assert!(result.is_ok());
    }

    #[test]
    fn test_cluster_topology_get_replicas() {
        let mut topology = ClusterTopology::new();
        let master = ClusterNode::new("master", "localhost", 6379);
        let replica1 = ClusterNode::new("replica1", "localhost", 6380).as_replica();
        let replica2 = ClusterNode::new("replica2", "localhost", 6381).as_replica();

        topology.add_node(master).unwrap();
        topology.add_node(replica1).unwrap();
        topology.add_node(replica2).unwrap();

        topology.add_replica("master", "replica1").unwrap();
        topology.add_replica("master", "replica2").unwrap();

        let replicas = topology.get_replicas("master");
        assert_eq!(replicas.len(), 2);
    }

    #[test]
    fn test_cluster_state_as_str() {
        assert_eq!(ClusterState::Ok.as_str(), "ok");
        assert_eq!(ClusterState::Partial.as_str(), "partial");
        assert_eq!(ClusterState::Failed.as_str(), "failed");
    }

    #[test]
    fn test_cluster_info_failed() {
        let topology = ClusterTopology::new();
        let info = ClusterInfo::new(&topology);
        assert_eq!(info.state, ClusterState::Failed);
    }

    #[test]
    fn test_cluster_info_partial() {
        let mut topology = ClusterTopology::new();
        let node = ClusterNode::new("node1", "localhost", 6379);
        topology.add_node(node).unwrap();
        topology.assign_slots("node1", vec![0, 100]).unwrap();

        let info = ClusterInfo::new(&topology);
        assert_eq!(info.state, ClusterState::Partial);
        assert_eq!(info.slots_assigned, 2);
    }

    #[test]
    fn test_cluster_node_get_address() {
        let node = ClusterNode::new("node1", "127.0.0.1", 6379);
        assert_eq!(node.get_address(), "127.0.0.1:6379");
    }

    #[test]
    fn test_extract_hash_tag() {
        assert_eq!(extract_hash_tag("{user:1}:profile"), "user:1");
        assert_eq!(extract_hash_tag("plain_key"), "plain_key");
    }
}
