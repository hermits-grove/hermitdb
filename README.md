<p align="center">
  <img src="art/george.svg"></img>
</p>

# HermitDB

######  A private decentralized database replicated over Git (or any other distributed log)

The replicated log datastructure has popped up in many distributed systems over the years, we see it in Bitcoin as the blockchain, we see it in systems that rely on distributed logs like Kafka, and of course we see it in Git as the branch commit history.

HermitDB recognizes the widespread deployment of these logs and will allow users to replicate their data using a log that they provide.

If this is all a bit too abstract, the motivating idea is that if you've built an app on HermitDB and I am a user of your app, I can sync the apps data across all of my devices by pointing your app to a Git repo that I control.

``` rust
extern crate hermitdb;
extern crate sled;

use hermitdb::{memory_log, map, DB};

fn main() {
	let actor = 32;
    let config = sled::ConfigBuilder::new().temporary(true).build();
    let tree = sled::Tree::start(config).unwrap();
    let log = memory_log::Log::new(actor);
    let map = map::Map::new(tree);
    let db = DB::new(log, map);
}
```

## If you have some spare attention, please direct it here.

- [ ] crypto
  - [ ] Reduce our reliance on a strong rng 
	  If an attacker controls our source of entropy, we may leak something.
    - [ ] Nonce's are generated randomly.
		Since we have a seperate encryption key per log, and logs are immutable (are they? what if we add compaction?) we should be able to use a counter on log entries as a nonce.
  - [ ] reduce our reliance on a strong password, all crypto entropy currently comes from the user's password, to improve security, we can look into adding an `entropy file`: a randomly generated file that is not synced through hermitdb. This would be similar to Keepass's composite key, the contents of the entropy file would be taken as input to the kdf giving us a lower bound of len(<entrop_file_content>) bits of entropy (assuming a strong rng was used to generate the entropy file).
- [ ] compressing op's in the log. Look into zstd (we already have zstd as a dependency from sled).
- [ ] log compaction
    1000 edits => 1000 log entries => 1000 commits (in the current git_log implementation).
    - [ ] Can we compact this log and preserve causality?
    - [ ] What if we make Op's themselves CRDT's `let op = merge(op1, op2)`?



## Prior Art

- https://github.com/ff-notes/ff - a distributed notes app built with CRDT's + Git

<p align="center">
  <img src="art/amanita.svg"></img>
</p>
