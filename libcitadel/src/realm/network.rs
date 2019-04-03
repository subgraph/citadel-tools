use std::path::{Path,PathBuf};
use std::net::Ipv4Addr;
use std::collections::{HashSet,HashMap};
use std::io::{BufReader,BufRead,Write};
use std::fs::{self,File};

use crate::Result;

const REALMS_RUN_PATH: &str = "/run/citadel/realms";

const CLEAR_BRIDGE_NETWORK: &str = "172.17.0.0/24";

const MIN_MASK: usize = 16;
const MAX_MASK: usize = 24;
const RESERVED_START: u8 = 200;

/// Manage ip address assignment for bridges
pub struct NetworkConfig {
    allocators: HashMap<String, BridgeAllocator>,
}

impl NetworkConfig {
    pub fn new() -> NetworkConfig {
        NetworkConfig {
            allocators: HashMap::new(),
        }
    }

    pub fn add_bridge(&mut self, name: &str, network: &str) -> Result<()> {
        let allocator = BridgeAllocator::for_bridge(name, network)
            .map_err(|e| format_err!("Failed to create bridge allocator: {}", e))?;
        self.allocators.insert(name.to_owned(), allocator);
        Ok(())
    }

    pub fn gateway(&self, bridge: &str) -> Result<String> {
        match self.allocators.get(bridge) {
            Some(allocator) => Ok(allocator.gateway()),
            None => bail!("Failed to return gateway address for bridge {} because it does not exist", bridge),
        }
    }

    pub fn allocate_address_for(&mut self, bridge: &str, realm_name: &str) -> Result<String> {
        match self.allocators.get_mut(bridge) {
            Some(allocator) => allocator.allocate_address_for(realm_name),
            None => bail!("Failed to allocate address for bridge {} because it does not exist", bridge),
        }
    }

    pub fn free_allocation_for(&mut self, bridge: &str, realm_name: &str) -> Result<()> {
        match self.allocators.get_mut(bridge) {
            Some(allocator) => allocator.free_allocation_for(realm_name),
            None => bail!("Failed to free address on bridge {} because it does not exist", bridge),
        }
    }

    pub fn allocate_reserved(&mut self, bridge: &str, realm_name: &str, octet: u8) -> Result<String> {
        match self.allocators.get_mut(bridge) {
            Some(allocator) => allocator.allocate_reserved(realm_name, octet),
            None => bail!("Failed to allocate address for bridge {} because it does not exist", bridge),
        }
    }
}

///
/// Allocates IP addresses for a bridge shared by multiple realms.
///
/// State information is stored in /run/citadel/realms/network-$bridge as
/// colon ':' separated pairs of realm name and allocated ip address
///
///    realm-a:172.17.0.2
///    realm-b:172.17.0.3
///
pub struct BridgeAllocator {
    bridge: String,
    network: Ipv4Addr,
    mask_size: usize,
    allocated: HashSet<Ipv4Addr>,
    allocations: HashMap<String, Ipv4Addr>,
}

impl BridgeAllocator {


    pub fn default_bridge() -> Result<BridgeAllocator> {
        BridgeAllocator::for_bridge("clear", CLEAR_BRIDGE_NETWORK)
    }

    pub fn for_bridge(bridge: &str, network: &str) -> Result<BridgeAllocator> {
        let (addr_str, mask_size) = match network.find('/') {
            Some(idx) => {
                let (net,bits) = network.split_at(idx);
                (net.to_owned(), bits[1..].parse()?)
            },
            None => (network.to_owned(), 24),
        };
        if mask_size > MAX_MASK || mask_size < MIN_MASK {
            bail!("Unsupported network mask size of {}", mask_size);
        }
        
        let mask = (1u32 << (32 - mask_size)) - 1;
        let ip = addr_str.parse::<Ipv4Addr>()?;

        if (u32::from(ip) & mask) != 0 {
            bail!("network {} has masked bits with netmask /{}", addr_str, mask_size);
        }

        let mut conf = BridgeAllocator::new(bridge, ip, mask_size);
        conf.load_state()?;
        Ok(conf)
    }

    fn new(bridge: &str, network: Ipv4Addr, mask_size: usize) -> BridgeAllocator {
        BridgeAllocator {
            bridge: bridge.to_owned(),
            allocated: HashSet::new(),
            allocations: HashMap::new(),
            network, mask_size,
        }
    }

    pub fn allocate_address_for(&mut self, realm_name: &str) -> Result<String> {
        match self.find_free_address() {
            Some(addr) => {
                self.allocated.insert(addr);
                if let Some(old) = self.allocations.insert(realm_name.to_owned(), addr) {
                    self.allocated.remove(&old);
                }
                self.write_state()?;
                Ok(format!("{}/{}", addr, self.mask_size))
            },
            None => bail!("No free IP address could be found to assign to {}", realm_name),
        }

    }

    fn store_allocation(&mut self, realm_name: &str, address: Ipv4Addr) -> Result<()> {
        self.allocated.insert(address);
        if let Some(old) = self.allocations.insert(realm_name.to_string(), address) {
            self.allocated.remove(&old);
        }
        self.write_state()
    }

    fn find_free_address(&self) -> Option<Ipv4Addr> {
        let mask = (1u32 << (32 - self.mask_size)) - 1;
        let net =  u32::from(self.network);
        for i in 2..mask {
            let addr = Ipv4Addr::from(net + i);
            if !Self::is_reserved(addr) && !self.allocated.contains(&addr) {
                return Some(addr);
            }
        }
        None
    }

    fn is_reserved(addr: Ipv4Addr) -> bool {
        addr.octets()[3] >= RESERVED_START
    }

    pub fn gateway(&self) -> String {
        let gw = u32::from(self.network) + 1;
        let addr = Ipv4Addr::from(gw);
        addr.to_string()
    }

    fn allocate_reserved(&mut self, realm_name: &str, octet: u8) -> Result<String> {
        if octet < RESERVED_START {
            bail!("Not a reserved octet: {}", octet);
        }
        let rsv = u32::from(self.network) | u32::from(octet);
        let addr = Ipv4Addr::from(rsv);
        let s = format!("{}/{}", addr, self.mask_size);
        if self.allocated.contains(&addr) {
            bail!("Already in use: {}", s);
        }
        self.store_allocation(realm_name, addr)?;
        Ok(s)
    }

    pub fn free_allocation_for(&mut self, realm_name: &str) -> Result<()> {
        match self.allocations.remove(realm_name) {
            Some(ip) =>  {
                self.allocated.remove(&ip);
                self.write_state()?;
            }
            None => warn!("No address allocation found for realm {}", realm_name),
        };
        Ok(())
    }

    fn state_file_path(&self) -> PathBuf {
        Path::new(REALMS_RUN_PATH).with_file_name(format!("network-{}", self.bridge))
    }


    fn load_state(&mut self) -> Result<()> {
        let path = self.state_file_path();
        if !path.exists() {
            return Ok(())
        }
        let f = File::open(path)?;
        let reader = BufReader::new(f);
        for line in reader.lines() {
            let line = &line?;
            self.parse_state_line(line)?;
        }

        Ok(())
    }

    fn parse_state_line(&mut self, line: &str) -> Result<()> {
        match line.find(':') {
            Some(idx) => {
                let (name,addr) = line.split_at(idx);
                let ip = addr[1..].parse::<Ipv4Addr>()?;
                self.allocated.insert(ip);
                self.allocations.insert(name.to_owned(), ip);
            },
            None => bail!("Could not parse line from network state file: {}", line),
        }
        Ok(())
    }

    fn write_state(&mut self) -> Result<()> {
        let path = self.state_file_path();
        let dir = path.parent().unwrap();
        if !dir.exists() {
            fs::create_dir_all(dir)
                .map_err(|e| format_err!("failed to create directory {} for network allocation state file: {}", dir.display(), e))?;
        }
        let mut f = File::create(&path)
            .map_err(|e| format_err!("failed to open network state file {} for writing: {}", path.display(), e))?;

        for (realm,addr) in &self.allocations {
            writeln!(f, "{}:{}", realm, addr)?;
        }
        Ok(())
    }
}
