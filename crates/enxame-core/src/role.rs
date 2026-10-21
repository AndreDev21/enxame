use serde::{Deserialize, Serialize};

/// Papel do nó na rede (spec seção 8.2). A ordem reflete a capacidade crescente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Role {
    Leaf,
    LeafPlus,
    Peer,
    Relay,
    SuperPeer,
}

#[cfg(test)]
mod tests {
    use super::Role;

    #[test]
    fn roles_are_ordered_by_capability() {
        assert!(Role::Leaf < Role::LeafPlus);
        assert!(Role::LeafPlus < Role::Peer);
        assert!(Role::Peer < Role::Relay);
        assert!(Role::Relay < Role::SuperPeer);
    }
}
