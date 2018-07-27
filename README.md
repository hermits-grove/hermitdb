<img align="middle" src="art/george.svg"></img>

# HermitDB

###### A privacy concious database for apps that respect user privacy 

#### Should you use gitdb?

Gitdb is not fast and it's features and tooling are bare bones, but... it may be enough for most apps.

Gitdb's goals is to be a fast-enough, offline-first, distributed key-value store with strong privacy and support for automated conflict resolution.

Applications built using GitDB tap into a large federated network for storing data: Git is a widely deployed and well understood protocol, users have a wide selection of storage options to choose from. They may pay Github, Gitlab, Bitbucket, etc.. or even host their own. 

The original motivator for gitdb was *mona,* a password manager which relied on Git to manage state across devices (*[the project is in development here.](https://github.com/the-gitdb-cooperative/mona)*)

*You should consider using gitdb if you care for user agancy over their data **and** you do not need to store obscene amounts of data or perform 100s of operations per second*

#### Design

GitDB stores all mutations to the database in git commits. These mutations are called `Ops` and they allow us to replicate the minimal amount of information necessary for other devices to stay in sync.

Everytime a mutating operation is performed on the database, we generate an `Op`. Op's themselves are also CRDT's meaning we can merge two Ops to generate a new Op which describe the mutations of both Ops.

Ops are not committed imediately, instead they are merged into a local cached Op. The cached Op is only committed when a git push is requested. This is done to avoid generating a very large number of commits.

When a new device is added to GitDB, we replay from the earliest known Op up to the most recent Op.

To sync, we perform a git fetch and apply all Ops from fetched commits in sequential order. We then commit our cached op (if one exists) and push it to the remote.

If by chance another device manages to commit and push an Op while we are attempting to push, we will now have a conflicting commit. To solve this we undo our own commit, put our uncommitted op back in the cached state, and restart the sync procedure. This repeats with exponential backoff until we push successfully (TAI: can we merge the conflicting Op with ours?)

##### Op

``` javascript
// example remove op
{
  "vclock": [
    {
      "actor": base64(98), // Actor chosen randomly, 128 bits to avoid collision
      "version": 33
    }
  ],
  "op": {
    "action": "remove",
    "key": base64("xyz".to_bytes()), // no key encoding restrictions, just bytes
    "type": "map"
  }
}

// Example Update op which sets "autologoff" to `false`.
//
// "autologoff" is an entry in the `prefs` map which, in turn, is an
// entry in the `mona` map.
{
  "vclock": [
    {
      "actor": base64(98), // Actor chosen randomly, 128 bits to avoid collision
      "version": 33
    }
  ],
  "op": {
    "action": "update",
    "key": base64("mona".to_bytes()),
    "type": "map",
    "op": { // Values are also CRDT's, nested op's structure depends on `type`
      "action": "update",
      "key": base64("pref".to_bytes()),
      "type": "map",
      "op": {
        "action": "update",
        "key": base64("autologoff".to_bytes()),
        "type": "reg",
        "op": {
          "val": { // TAI: registers can hold any type of data?
            "type": "bool",
            "data": false
          }
        }
      }
    }
  }
}
```

## Prior Art

- https://github.com/ff-notes/ff - a distributed notes app built with CRDT's + Git

<div style="text-align: center">
  <object data="art/amanita.svg" type="image/svg+xml"> 
    <img src="art/amanita.jpg" /> <!-- TODO: need to render a fallback jpg!!! -->
  </object>
</div>
