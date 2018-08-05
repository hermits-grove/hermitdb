<p align="center">
  <img src="art/george.svg"></img>
</p>

# HermitDB

######  A private decentralized database replicated over Git (or any other distributed log)

The replicated log datastructure has popped up in many distributed systems over the years, we see it in Bitcoin as the blockchain, we see it in systems that rely on distributed logs like Kafka, and of course we see it in Git as the branch commit history.

HermitDB recognizes the whitespread deployment of these logs and will allow users to replicate their data using a log that they provide.

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

## Motivation

We are now seeing the dangers of centralized data and many of us are looking for and building decentralized alternatives to the tools we use.

The larger scale problems seem to be in good hands. Decentralized solutions are popping up everyday for social networks, money, content distribution, online identity and many other hard problems.

<p align="center">
	But what about the tools to manage your life?
</p>

Tools like password managers, calendars, contact books and note taking apps. These all help us organize our life. Unfortunatly, we give up our data when we want to sync across our devices.

Developers want to give us an experience where our data follows us around, but the existing infrastructure and tooling push developers in the direction of centralized data.
<p align="center">
	This is where HermitDB want's to help out.
	<br>
	<br>
	<i>Tools built with HermitDB give users agency over their data.</i>
</p>
	
## In The Weeds

At it's core, HermitDB is a Key/Value CmRDT store where ops are replicated over a user provided log. Values in the key/value store are themselves also CmRDT's.

## Prior Art

- https://github.com/ff-notes/ff - a distributed notes app built with CRDT's + Git

<p align="center">
  <img src="art/amanita.svg"></img>
</p>
