
type Actor = u128;
type Version = u64;

enum Action {
    Update {
        dot: Dot,
        key: Vec<u8>,
        type: KeyType,
        
    },
    Remove
}

struct Dot {
    actor: Actor,
    version: Version
}

enum Update {
    Map,
    Reg
}

struct Op {
    action: Action,
    dot: Dot,
    key: Vec<u8>,
    type: KeyType,
    
}
