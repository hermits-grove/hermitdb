<p align="center">
  <img src="art/george.svg"></img>
</p>

# HermitDB

######  A private decentralized database replicated over Git (or any other distributed log)

The replicated log abstraction has popped up in many distributed systems over the years, we see it in Bitcoin as the blockchain, we see it in systems that rely on distributed logs like Kafka, and we see it in Git as the branch commit history.

HermitDB uses the widespread deployment of these logs to give users the ability to replicate their data using a log that they have access to.

The motivating idea behind HermitDB is that if you've built a password manager with HermitDB, users of this password manager can effortlesly sync their data across their devices using a git repo they control, meaning they keep control over their data.

``` rust
extern crate hermitdb;

use hermitdb::{sled, memory_log, map, DB};

fn main() {
    let actor = 32;
    let config = sled::ConfigBuilder::new().temporary(true).build();
    let tree = sled::Tree::start(config).unwrap();
    // use an in memory log for testing
    let log = memory_log::Log::new(actor);
    let map = map::Map::new(tree);
    let db = DB::new(log, map);
}
```

### If you've got some spare time...

- **crypto**
  - **Reduce our reliance on a strong rng**
	- If an attacker controls our source of entropy, it increases chance of leak.
    - Nonce's are currently generated randomly. Since we have a seperate encryption key per log, and logs are immutable (are they? what if we add compaction?) we should be able to use a counter on log entries as a nonce.
  - **reduce our reliance on a strong password.** The users password is the weakest link in our crypto system. To improve security, we can look into adding an `entropy file`: a randomly generated file that is not synced through hermitdb. This would be similar to Keepass's composite key, the contents of the entropy file would be taken as input to the kdf giving us a lower bound of `len(<entrop_file_content>)` bits of entropy (assuming a strong rng was used to generate the entropy file).
- **compressing op's in the log**
	- Look into zstd (we already have zstd as a dependency from sled).
- **log compaction**
    - 1000 edits => 1000 log entries => 1000 commits (in the current git_log implementation).
    - Can we compact this log *and* preserve causality?
    - can we make `Op`'s themselves CRDT's? `let compacted_op = merge(op1, op2)`



### Prior Art

- https://github.com/ff-notes/ff - a distributed notes app built with CRDT's + Git

<p align="center">
  <img src="art/amanita.svg"></img>
</p>
